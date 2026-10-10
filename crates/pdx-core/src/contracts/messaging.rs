//! Channel contracts from broker client calls (4.7.1's producer calls and consumer
//! calls): Spring's `KafkaTemplate`, Kafka's own clients (Java, `kafka-python`,
//! `confluent-kafka`, `kafkajs`), AMQP (`pika`, `amqplib`), NATS, Redis publish
//! (`redis-py`, `ioredis`, go-redis, Jedis, Lettuce, Spring's `RedisTemplate`) and JMS
//! (Spring's `JmsTemplate`, and a `MessageProducer` given its destination inline).
//!
//! Each [`Rule`] is a library's operation: the modules a file must import, the
//! operation's name, the receiver types it belongs to, and where its destination is.
//! A call is the operation only by provenance and shape ([`super::calls`]): the file
//! imports the library, the operation has the destination as a literal (or a
//! `${KEY}` with one configuration value) where the library takes it, a receiver whose
//! declared type the facts give is the library's, and resolution does not send the call
//! to a definition of the repository. When rules of different meaning match one call,
//! it is ambiguous and gives nothing. Kotlin calls carry no arguments in the extracted
//! facts, so no Kotlin call is read (issue 63).

use std::collections::BTreeSet;

use pdx_engine::{Call, SiteRef};

use crate::index::derive::DeriveError;
use crate::index::derive::calls::{call_site, ordinals};
use crate::kinds::SiteKind;
use crate::model::ContractDirection;
use crate::resolve::stages::split_callee;

use super::annotation::string_literal;
use super::calls::{
    constructed_first, factory_literal, imports_any, is_internal, keyword_or, literal,
    literal_list, object_value, positional, receiver_may_be,
};
use super::channels::channel;
use super::{Context, Found};

/// Where an operation takes its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// The literal argument at a position.
    Arg(u32),
    /// The literal first argument of a constructor of one of these types, given as the
    /// first argument (`new ProducerRecord<>("orders", …)`).
    Constructed(&'static [&'static str]),
    /// The literal argument of one of these factories, given as the first argument
    /// (`session.createQueue("orders")`).
    Factory(&'static [&'static str]),
    /// A list of literals as the first argument.
    List,
    /// An options object's `topic`, or its `topics` list, as the first argument.
    TopicObject,
    /// AMQP: the exchange (a keyword or a position) when it is not empty, else the
    /// routing key, which is then the queue's name.
    Amqp {
        /// The exchange's keyword and position.
        exchange: (&'static str, u32),
        /// The routing key's keyword and position.
        routing: (&'static str, u32),
    },
}

/// A library's producer or consumer operation.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    /// The client, as evidence (`KafkaTemplate.send`).
    pub client: &'static str,
    /// The transport, as evidence: `kafka`, `rabbitmq`, `nats`, `redis`, `jms`.
    pub transport: &'static str,
    /// The languages it is read in.
    pub languages: &'static [&'static str],
    /// The modules a file must import one of.
    pub modules: &'static [&'static str],
    /// The operation's names.
    pub methods: &'static [&'static str],
    /// The receiver's types, when the facts declare it.
    pub types: &'static [&'static str],
    /// The fewest arguments the operation takes with an explicit destination.
    pub min_args: usize,
    /// Where the destination is.
    pub destination: Destination,
    /// Publish or subscribe.
    pub direction: ContractDirection,
}

const JAVA: &[&str] = &["java"];
const PYTHON: &[&str] = &["python"];
const SCRIPT: &[&str] = &["javascript", "typescript"];
const GO: &[&str] = &["go"];

/// The operations read.
pub const RULES: &[Rule] = &[
    Rule {
        client: "KafkaTemplate.send",
        transport: "kafka",
        languages: JAVA,
        modules: &["org.springframework.kafka"],
        methods: &["send"],
        types: &["KafkaTemplate"],
        min_args: 2,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "KafkaProducer.send",
        transport: "kafka",
        languages: JAVA,
        modules: &["org.apache.kafka.clients.producer"],
        methods: &["send"],
        types: &["KafkaProducer", "Producer"],
        min_args: 1,
        destination: Destination::Constructed(&["ProducerRecord"]),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "KafkaConsumer.subscribe",
        transport: "kafka",
        languages: JAVA,
        modules: &["org.apache.kafka.clients.consumer"],
        methods: &["subscribe"],
        types: &["KafkaConsumer", "Consumer"],
        min_args: 1,
        destination: Destination::List,
        direction: ContractDirection::Consumes,
    },
    Rule {
        client: "kafka-python producer.send",
        transport: "kafka",
        languages: PYTHON,
        modules: &["kafka", "aiokafka"],
        methods: &["send", "send_and_wait"],
        types: &[],
        min_args: 1,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "kafka-python consumer.subscribe",
        transport: "kafka",
        languages: PYTHON,
        modules: &["kafka", "aiokafka"],
        methods: &["subscribe"],
        types: &[],
        min_args: 1,
        destination: Destination::List,
        direction: ContractDirection::Consumes,
    },
    Rule {
        client: "confluent-kafka produce",
        transport: "kafka",
        languages: PYTHON,
        modules: &["confluent_kafka"],
        methods: &["produce"],
        types: &[],
        min_args: 1,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "confluent-kafka subscribe",
        transport: "kafka",
        languages: PYTHON,
        modules: &["confluent_kafka"],
        methods: &["subscribe"],
        types: &[],
        min_args: 1,
        destination: Destination::List,
        direction: ContractDirection::Consumes,
    },
    Rule {
        client: "kafkajs producer.send",
        transport: "kafka",
        languages: SCRIPT,
        modules: &["kafkajs"],
        methods: &["send"],
        types: &[],
        min_args: 1,
        destination: Destination::TopicObject,
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "kafkajs consumer.subscribe",
        transport: "kafka",
        languages: SCRIPT,
        modules: &["kafkajs"],
        methods: &["subscribe"],
        types: &[],
        min_args: 1,
        destination: Destination::TopicObject,
        direction: ContractDirection::Consumes,
    },
    Rule {
        client: "pika basic_publish",
        transport: "rabbitmq",
        languages: PYTHON,
        modules: &["pika"],
        methods: &["basic_publish"],
        types: &[],
        min_args: 2,
        destination: Destination::Amqp {
            exchange: ("exchange", 0),
            routing: ("routing_key", 1),
        },
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "amqplib sendToQueue",
        transport: "rabbitmq",
        languages: SCRIPT,
        modules: &["amqplib", "amqp-connection-manager"],
        methods: &["sendToQueue"],
        types: &[],
        min_args: 2,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "amqplib publish",
        transport: "rabbitmq",
        languages: SCRIPT,
        modules: &["amqplib", "amqp-connection-manager"],
        methods: &["publish"],
        types: &[],
        min_args: 3,
        destination: Destination::Amqp {
            exchange: ("", 0),
            routing: ("", 1),
        },
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "nats publish",
        transport: "nats",
        languages: SCRIPT,
        modules: &["nats"],
        methods: &["publish"],
        types: &[],
        min_args: 1,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "nats-py publish",
        transport: "nats",
        languages: PYTHON,
        modules: &["nats"],
        methods: &["publish"],
        types: &[],
        min_args: 1,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "nats.go Publish",
        transport: "nats",
        languages: GO,
        modules: &["github.com/nats-io/nats.go"],
        methods: &["Publish"],
        types: &[],
        min_args: 1,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "redis-py publish",
        transport: "redis",
        languages: PYTHON,
        modules: &["redis", "aioredis"],
        methods: &["publish"],
        types: &[],
        min_args: 2,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "ioredis publish",
        transport: "redis",
        languages: SCRIPT,
        modules: &["ioredis", "redis"],
        methods: &["publish"],
        types: &[],
        min_args: 2,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "go-redis Publish",
        transport: "redis",
        languages: GO,
        modules: &["github.com/redis/go-redis", "github.com/go-redis/redis"],
        methods: &["Publish"],
        types: &[],
        min_args: 3,
        destination: Destination::Arg(1),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "Jedis publish",
        transport: "redis",
        languages: JAVA,
        modules: &["redis.clients.jedis", "io.lettuce.core"],
        methods: &["publish"],
        types: &[
            "Jedis",
            "JedisPooled",
            "UnifiedJedis",
            "JedisCluster",
            "RedisCommands",
            "RedisAsyncCommands",
            "RedisReactiveCommands",
        ],
        min_args: 2,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "RedisTemplate.convertAndSend",
        transport: "redis",
        languages: JAVA,
        modules: &["org.springframework.data.redis"],
        methods: &["convertAndSend"],
        types: &[
            "RedisTemplate",
            "StringRedisTemplate",
            "ReactiveRedisTemplate",
            "ReactiveStringRedisTemplate",
            "RedisOperations",
        ],
        min_args: 2,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "JmsTemplate.convertAndSend",
        transport: "jms",
        languages: JAVA,
        modules: &["org.springframework.jms"],
        methods: &["convertAndSend", "send"],
        types: &["JmsTemplate", "JmsMessagingTemplate", "JmsOperations"],
        min_args: 2,
        destination: Destination::Arg(0),
        direction: ContractDirection::Provides,
    },
    Rule {
        client: "JMS MessageProducer.send",
        transport: "jms",
        languages: JAVA,
        modules: &["javax.jms", "jakarta.jms"],
        methods: &["send"],
        types: &[
            "MessageProducer",
            "JMSProducer",
            "QueueSender",
            "TopicPublisher",
        ],
        min_args: 2,
        destination: Destination::Factory(&["createQueue", "createTopic"]),
        direction: ContractDirection::Provides,
    },
];

/// The destinations a call gives under a rule, as written; `None` when it is not the
/// rule's operation or its destination is not a literal.
fn destinations(rule: &Rule, call: &Call) -> Option<Vec<String>> {
    let (receiver, name) = split_callee(&call.callee_text);
    if receiver.is_none() || !rule.methods.contains(&name) || call.args.len() < rule.min_args {
        return None;
    }
    let first = || positional(call, 0);
    let found = match rule.destination {
        Destination::Arg(i) => vec![literal(positional(call, i)?)?],
        Destination::Constructed(types) => vec![constructed_first(&first()?.expr, types)?],
        Destination::Factory(factories) => vec![factory_literal(&first()?.expr, factories)?],
        Destination::List => literal_list(&first()?.expr)?,
        Destination::TopicObject => {
            let expr = &first()?.expr;
            match (object_value(expr, "topic"), object_value(expr, "topics")) {
                (Some(topic), None) => vec![string_literal(topic)?],
                (None, Some(topics)) => literal_list(topics)?,
                _ => return None,
            }
        }
        Destination::Amqp { exchange, routing } => {
            let exchange = literal(keyword_or(call, exchange.0, exchange.1)?)?;
            if exchange.is_empty() {
                vec![literal(keyword_or(call, routing.0, routing.1)?)?]
            } else {
                vec![exchange]
            }
        }
    };
    (!found.is_empty() && found.iter().all(|d| !d.is_empty())).then_some(found)
}

/// One call's reading: the rule's meaning and the destinations.
type Reading = (&'static str, &'static str, &'static str, Vec<String>);

pub(crate) fn observe(context: &Context<'_>, found: &mut Found) -> Result<(), DeriveError> {
    let registry = context.registry;
    let ordinals = ordinals(registry);
    let kafka: BTreeSet<String> = context.config.kafka_cluster().into_iter().collect();
    let rabbit: BTreeSet<String> = context.config.rabbit_broker().into_iter().collect();
    let none = BTreeSet::new();
    for path in registry.files() {
        let (Some(language), Some(extract)) = (registry.language(path), registry.extract(path))
        else {
            continue;
        };
        let rules: Vec<&Rule> = RULES
            .iter()
            .filter(|r| {
                r.languages.contains(&language.id) && imports_any(registry, path, r.modules)
            })
            .collect();
        if rules.is_empty() {
            continue;
        }
        let file = context.graph.file_node(path)?.clone();
        for (index, call) in extract.calls.iter().enumerate() {
            if call.is_reference || is_internal(context, path, index) {
                continue;
            }
            let readings: BTreeSet<Reading> = rules
                .iter()
                .filter(|r| receiver_may_be(registry, path, call, r.types))
                .filter_map(|r| {
                    destinations(r, call).map(|d| (r.client, r.transport, r.direction.as_str(), d))
                })
                .collect();
            // Rules of different meaning on one call: ambiguous, nothing.
            let meanings: BTreeSet<_> = readings
                .iter()
                .map(|(_, transport, direction, destinations)| {
                    (*transport, *direction, destinations)
                })
                .collect();
            let [(transport, direction, destinations)] =
                meanings.into_iter().collect::<Vec<_>>()[..]
            else {
                continue;
            };
            let Some(direction) = ContractDirection::parse(direction) else {
                continue;
            };
            let clients: BTreeSet<&str> = readings.iter().map(|(client, ..)| *client).collect();
            let site_ref = SiteRef {
                rel_path: path.to_owned(),
                call_index: u32::try_from(index).unwrap_or(u32::MAX),
            };
            found.covered.insert(site_ref.clone());
            let site = match call_site(
                context.graph,
                registry,
                &ordinals,
                &site_ref,
                call,
                SiteKind::Call,
            )? {
                Ok((site, _)) if context.graph.sites.contains_key(&site.site_id) => {
                    Some(site.site_id.as_str().to_owned())
                }
                _ => None,
            };
            let exact = match transport {
                "kafka" => &kafka,
                "rabbitmq" => &rabbit,
                _ => &none,
            };
            for destination in destinations {
                let raw = format!("{}({destination})", call.callee_text);
                let Some(mut observation) =
                    channel(context, destination, direction, path, raw, exact)
                else {
                    continue;
                };
                observation = observation.with_evidence("transports", transport);
                for client in &clients {
                    observation = observation.with_evidence("clients", *client);
                }
                if direction == ContractDirection::Provides {
                    observation = observation.with_owner(file.clone());
                }
                if let Some(site) = &site {
                    observation = observation.with_evidence("site_ids", site.clone());
                }
                found.observe(observation);
            }
        }
    }
    Ok(())
}
