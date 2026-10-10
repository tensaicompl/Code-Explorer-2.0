//! Channel contracts (4.7.1): the topics and queues a repository publishes to and
//! listens on.
//!
//! **The engine's channel facts** (`FileExtract::channels`): an `Emit` provides its
//! channel, owned by its file (the facts carry no position, so no narrower owner is
//! safe); a `Listen` consumes it. The engine labels more than destinations: a socket's
//! messages and an in-process event emitter's events are channel facts too, and a
//! socket with no named channel is labelled with its function's name. A fact is used
//! only when a broker client call in its file names it: a call on a `producer`,
//! `consumer` or (AMQP) `channel` receiver, of an operation of the fact's direction
//! (`send`, `produce`, `publish`, `basic_publish`, `sendToQueue`, `sendBatch`; or
//! `subscribe`, `poll`, `consume`, `basic_consume`), with an argument that is the
//! channel. Any other is reported (issue 61).
//!
//! **Listener annotations**: `@KafkaListener(topics = …)`, `@RabbitListener(queues =
//! …)` and `@JmsListener(destination = …)` on a definition consume each literal or
//! placeholder destination they name.
//!
//! **`AsyncAPI` documents** (`asyncapi*.{yaml,yml,json}`): every channel is provided by
//! the document's file, or consumed when the document says the application receives
//! from it (2.x's `publish`, 3.x's `receive`); 2.x's `subscribe` and 3.x's `send`
//! provide it. Only references inside the document are followed.
//!
//! A destination is case-sensitive and kept as written. A `${KEY}` placeholder takes
//! its single configuration value; otherwise the key is `unresolved:` and the
//! placeholder, and the placeholder is kept in `props.placeholders`. The broker is the
//! namespace: the repository's declared `brokers`; a literal Kafka cluster or `RabbitMQ`
//! broker in its configuration for a listener of that transport; an `AsyncAPI`
//! document's literal servers. A transport's name is never a broker.

use std::collections::BTreeSet;

use pdx_engine::{Call, ChannelDirection, SiteRef};

use crate::index::derive::DeriveError;
use crate::index::derive::calls::{call_site, ordinals};
use crate::kinds::{ContractKind, SiteKind};
use crate::model::ContractDirection;
use crate::resolve::stages::split_callee;

use super::annotation::{self, string_literal};
use super::calls::imports_any;
use super::config_values::Resolved;
use super::document::{self, Doc, DocError};
use super::identity::{host_port, unresolved_channel_key};
use super::source::{self, Read};
use super::{
    Context, ContractObservation, ContractProblem, DeclaredIdentities, Found, extension, file_name,
};

/// Operations that publish to a broker destination.
const EMIT_OPERATIONS: [&str; 6] = [
    "send",
    "sendBatch",
    "produce",
    "publish",
    "basic_publish",
    "sendToQueue",
];

/// Operations that consume a broker destination.
const LISTEN_OPERATIONS: [&str; 4] = ["subscribe", "poll", "consume", "basic_consume"];

/// The receivers the engine classifies a channel fact's client by, and the libraries
/// one of which the file must import for the classification to stand: the receiver's
/// name alone is never evidence (issue 62).
const BROKER_RECEIVERS: [(&str, &[&str]); 3] = [
    ("producer", KAFKA_LIBRARIES),
    ("consumer", KAFKA_LIBRARIES),
    ("channel", AMQP_LIBRARIES),
];

/// Kafka client libraries.
const KAFKA_LIBRARIES: &[&str] = &[
    "kafka",
    "aiokafka",
    "confluent_kafka",
    "kafkajs",
    "kafka-node",
    "node-rdkafka",
];

/// AMQP client libraries.
const AMQP_LIBRARIES: &[&str] = &["pika", "aio_pika", "amqplib", "amqp-connection-manager"];

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    engine_facts(context, found)?;
    annotations(context, found);
    asyncapi(context, found)
}

/// A destination's key, its placeholder if it had one, or `None` for an empty one.
fn destination(context: &Context<'_>, raw: &str) -> Option<(String, Option<String>)> {
    if raw.is_empty() {
        return None;
    }
    Some(match context.config.resolve(raw) {
        Resolved::Literal(text) => (text, None),
        Resolved::Placeholder { raw, value } => (value, Some(raw)),
        Resolved::Unresolved(raw) => (unresolved_channel_key(&raw), Some(raw)),
    })
}

/// A channel observation with its key, placeholder and identities set.
pub(crate) fn channel(
    context: &Context<'_>,
    raw: &str,
    direction: ContractDirection,
    path: &str,
    raw_form: String,
    exact: &BTreeSet<String>,
) -> Option<ContractObservation> {
    let (key, placeholder) = destination(context, raw)?;
    let mut observation =
        ContractObservation::new(ContractKind::Channel, key, direction, path, raw_form)
            .with_identities(DeclaredIdentities::with_exact(
                exact,
                &context.declared.brokers,
            ));
    if let Some(placeholder) = placeholder {
        observation = observation.with_evidence("placeholders", placeholder);
    }
    Some(observation)
}

/// Whether a call is a broker client call of `direction` naming `channel`, in a file
/// that imports the client's library.
fn names_channel(
    registry: &crate::resolve::registry::SymbolRegistry,
    path: &str,
    call: &Call,
    direction: ChannelDirection,
    channel: &str,
) -> bool {
    let (Some(receiver), name) = split_callee(&call.callee_text) else {
        return false;
    };
    let tail = receiver.rsplit(['.', ':']).next().unwrap_or(receiver);
    let operations: &[&str] = match direction {
        ChannelDirection::Emit => &EMIT_OPERATIONS,
        ChannelDirection::Listen => &LISTEN_OPERATIONS,
    };
    let Some((_, libraries)) = BROKER_RECEIVERS.iter().find(|(r, _)| *r == tail) else {
        return false;
    };
    if call.is_reference || !operations.contains(&name) || !imports_any(registry, path, libraries) {
        return false;
    }
    names(call, channel)
}

/// Whether one of a call's arguments is the channel, or a list holding it.
fn names(call: &Call, channel: &str) -> bool {
    let quoted = [format!("\"{channel}\""), format!("'{channel}'")];
    call.args.iter().any(|a| {
        a.value.as_deref() == Some(channel)
            || string_literal(&a.expr).as_deref() == Some(channel)
            || quoted.iter().any(|q| a.expr.contains(q.as_str()))
    })
}

fn engine_facts(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    let ordinals = ordinals(registry);
    for path in registry.files() {
        let Some(extract) = registry.extract(path) else {
            continue;
        };
        if extract.channels.is_empty() {
            continue;
        }
        let file = context.graph.file_node(path)?.clone();
        for fact in &extract.channels {
            // A broker client rule already read the call that carries this fact.
            let read = extract.calls.iter().enumerate().any(|(i, c)| {
                found.covered.contains(&SiteRef {
                    rel_path: path.to_owned(),
                    call_index: u32::try_from(i).unwrap_or(u32::MAX),
                }) && names(c, &fact.channel_text)
            });
            if read {
                continue;
            }
            let calls: Vec<(usize, &Call)> = extract
                .calls
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    names_channel(registry, path, c, fact.direction, &fact.channel_text)
                })
                .collect();
            if calls.is_empty() {
                found.diagnose(path, ContractProblem::UnconfirmedChannel);
                continue;
            }
            let direction = match fact.direction {
                ChannelDirection::Emit => ContractDirection::Provides,
                ChannelDirection::Listen => ContractDirection::Consumes,
            };
            for (index, call) in calls {
                let raw_form = format!("{}({})", call.callee_text, fact.channel_text);
                let Some(mut observation) = channel(
                    context,
                    &fact.channel_text,
                    direction,
                    path,
                    raw_form,
                    &BTreeSet::new(),
                ) else {
                    continue;
                };
                observation = observation.with_evidence("clients", call.callee_text.clone());
                if direction == ContractDirection::Provides {
                    observation = observation.with_owner(file.clone());
                }
                let site_ref = SiteRef {
                    rel_path: path.to_owned(),
                    call_index: u32::try_from(index).unwrap_or(u32::MAX),
                };
                if let Ok((site, _)) = call_site(
                    context.graph,
                    registry,
                    &ordinals,
                    &site_ref,
                    call,
                    SiteKind::Call,
                )? && context.graph.sites.contains_key(&site.site_id)
                {
                    observation = observation.with_evidence("site_ids", site.site_id.as_str());
                }
                found.observe(observation);
            }
        }
    }
    Ok(())
}

fn annotations(context: &Context<'_>, found: &mut Found) {
    let registry = context.registry;
    let kafka: BTreeSet<String> = context.config.kafka_cluster().into_iter().collect();
    let rabbit: BTreeSet<String> = context.config.rabbit_broker().into_iter().collect();
    let none = BTreeSet::new();
    for path in registry.files() {
        let Some(extract) = registry.extract(path) else {
            continue;
        };
        let interpolates = registry
            .language(path)
            .is_some_and(|l| matches!(l.id, "kotlin" | "groovy"));
        for definition in &extract.definitions {
            for decorator in &definition.decorators {
                let Some((name, args)) = annotation::parse_in(decorator, interpolates) else {
                    continue;
                };
                let (transport, keys, exact) = match name.as_str() {
                    "KafkaListener" => ("kafka", &["topics"][..], &kafka),
                    "RabbitListener" => ("rabbitmq", &["queues"][..], &rabbit),
                    "JmsListener" => ("jms", &["destination"][..], &none),
                    _ => continue,
                };
                let Some(destinations) =
                    annotation::named(&args, keys, false).and_then(annotation::Arg::strings)
                else {
                    continue;
                };
                let raw_form = decorator.split_whitespace().collect::<Vec<_>>().join(" ");
                for raw in destinations {
                    if let Some(observation) = channel(
                        context,
                        raw,
                        ContractDirection::Consumes,
                        path,
                        raw_form.clone(),
                        exact,
                    ) {
                        found.observe(observation.with_evidence("transports", transport));
                    }
                }
            }
        }
    }
}

/// Whether a file is an `AsyncAPI` document by its name.
pub fn is_asyncapi(path: &str) -> bool {
    file_name(path).starts_with("asyncapi")
        && matches!(extension(path), Some("yaml" | "yml" | "json"))
}

fn asyncapi(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    for path in registry.files() {
        if !is_asyncapi(path) {
            continue;
        }
        let text = match source::read(context.root, registry, path)? {
            Read::Text(text) => text,
            Read::NotUtf8 => {
                found.diagnose(path, ContractProblem::NotUtf8);
                continue;
            }
            Read::Unread => continue,
        };
        let json = extension(path) == Some("json");
        let parsed = if json {
            document::parse_json(&text).map(|d| vec![d])
        } else {
            document::parse_yaml(&text)
        };
        let doc = match parsed {
            Ok(mut docs) if docs.len() == 1 => docs.remove(0),
            Ok(_) | Err(DocError::Syntax) => {
                let format = if json { "json" } else { "yaml" };
                found.diagnose(path, ContractProblem::Unparseable { format });
                continue;
            }
            Err(DocError::TooDeep) => {
                found.diagnose(path, ContractProblem::TooDeep);
                continue;
            }
        };
        let Some(version) = doc.get("asyncapi").and_then(Doc::as_str) else {
            found.diagnose(path, ContractProblem::NotADocument { format: "asyncapi" });
            continue;
        };
        let file = context.graph.file_node(path)?.clone();
        let servers = servers(&doc);
        let bindings = if version.starts_with("2.") {
            version_2(&doc, path, found)
        } else {
            version_3(&doc, path, found)
        };
        for (name, direction, raw_form) in bindings {
            let Some(mut observation) =
                channel(context, &name, direction, path, raw_form, &servers)
            else {
                continue;
            };
            observation = observation.with_evidence("frameworks", "asyncapi");
            if direction == ContractDirection::Provides {
                observation = observation.with_owner(file.clone());
            }
            found.observe(observation);
        }
    }
    Ok(())
}

/// A document's literal servers, canonical; a templated one gives none.
fn servers(doc: &Doc) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, server) in doc.get("servers").map(Doc::entries).into_iter().flatten() {
        let Some(text) = server
            .get("host")
            .or_else(|| server.get("url"))
            .and_then(Doc::as_str)
        else {
            continue;
        };
        let authority = text.split_once("://").map_or(text, |(_, rest)| rest);
        let authority = authority.split('/').next().unwrap_or(authority);
        if let Some(host) = host_port(authority, None) {
            out.insert(host);
        }
    }
    out
}

/// 2.x: each channel, by its operations (`subscribe`: the application sends;
/// `publish`: it receives), or provided when it has neither.
fn version_2(doc: &Doc, path: &str, found: &mut Found) -> Vec<(String, ContractDirection, String)> {
    let mut out = Vec::new();
    for (name, item) in doc.get("channels").map(Doc::entries).into_iter().flatten() {
        if item.get("$ref").is_some() {
            found.diagnose(path, ContractProblem::UnfollowedRef);
            continue;
        }
        let sends = item.get("subscribe").is_some();
        let receives = item.get("publish").is_some();
        if sends || !receives {
            let operation = if sends { "subscribe" } else { "channel" };
            out.push((
                name.to_owned(),
                ContractDirection::Provides,
                format!("{operation} {name}"),
            ));
        }
        if receives {
            out.push((
                name.to_owned(),
                ContractDirection::Consumes,
                format!("publish {name}"),
            ));
        }
    }
    out
}

/// 3.x: each operation's channel (`send` provides, `receive` consumes), referenced as
/// `#/channels/<id>`, at its `address`; a channel no operation uses is provided. A
/// channel with no literal address names no destination.
fn version_3(doc: &Doc, path: &str, found: &mut Found) -> Vec<(String, ContractDirection, String)> {
    let channels = doc.get("channels");
    let address = |id: &str| {
        channels
            .and_then(|c| c.get(id))
            .and_then(|c| c.get("address"))
            .and_then(Doc::as_str)
            .map(str::to_owned)
    };
    let mut out = Vec::new();
    let mut used = BTreeSet::new();
    for (_, operation) in doc
        .get("operations")
        .map(Doc::entries)
        .into_iter()
        .flatten()
    {
        let direction = match operation.get("action").and_then(Doc::as_str) {
            Some("send") => ContractDirection::Provides,
            Some("receive") => ContractDirection::Consumes,
            _ => continue,
        };
        let Some(reference) = operation
            .get("channel")
            .and_then(|c| c.get("$ref"))
            .and_then(Doc::as_str)
        else {
            continue;
        };
        let Some(id) = reference.strip_prefix("#/channels/") else {
            found.diagnose(path, ContractProblem::UnfollowedRef);
            continue;
        };
        let id = id.replace("~1", "/").replace("~0", "~");
        used.insert(id.clone());
        if let Some(name) = address(&id) {
            let action = if direction == ContractDirection::Provides {
                "send"
            } else {
                "receive"
            };
            out.push((name.clone(), direction, format!("{action} {name}")));
        }
    }
    for (id, _) in channels.map(Doc::entries).into_iter().flatten() {
        if !used.contains(id)
            && let Some(name) = address(id)
        {
            out.push((
                name.clone(),
                ContractDirection::Provides,
                format!("channel {name}"),
            ));
        }
    }
    out
}
