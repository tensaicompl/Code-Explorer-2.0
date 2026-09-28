/*
 * service_patterns.c — Classify call edges by library identity in resolved QN.
 *
 * Instead of matching callee names (ambiguous: "get", "post", "send"),
 * we match library identifiers in the RESOLVED qualified name. The QN
 * contains the full module path, so import aliases are transparent:
 *   r.get("/api") → QN: project.venv.requests.api.get → match "requests" → HTTP_CALLS
 *
 * Two-level matching:
 *   1. Library identifier in QN → determines edge type (HTTP/ASYNC/CONFIG)
 *   2. Method suffix → determines HTTP method (get→GET, post→POST)
 */
#include "service_patterns.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

/* ── Library identifier → edge type ────────────────────────────── */

typedef struct {
    const char *library_id; /* substring to find in resolved QN */
    pdxe_svc_kind_t kind;    /* HTTP_CALLS, ASYNC_CALLS, CONFIGURES */
    const char *broker;     /* for ASYNC: broker name (NULL otherwise) */
} lib_pattern_t;

/* HTTP client libraries — match these substrings in the resolved QN.
 * Sources: github.com/easybase/awesome-http, official SDK docs, agent research */
static const lib_pattern_t http_libraries[] = {
    /* Python */
    {"requests", PDXE_SVC_HTTP, NULL},
    {"httpx", PDXE_SVC_HTTP, NULL},
    {"aiohttp", PDXE_SVC_HTTP, NULL},
    {"urllib", PDXE_SVC_HTTP, NULL},
    {"urllib3", PDXE_SVC_HTTP, NULL},
    {"httplib2", PDXE_SVC_HTTP, NULL},
    {"pycurl", PDXE_SVC_HTTP, NULL},
    {"treq", PDXE_SVC_HTTP, NULL},
    {"uplink", PDXE_SVC_HTTP, NULL},

    /* JavaScript / TypeScript */
    {"axios", PDXE_SVC_HTTP, NULL},
    {"superagent", PDXE_SVC_HTTP, NULL},
    {"needle", PDXE_SVC_HTTP, NULL},
    {"node-fetch", PDXE_SVC_HTTP, NULL},
    {"undici", PDXE_SVC_HTTP, NULL},
    {"ofetch", PDXE_SVC_HTTP, NULL},
    {"wretch", PDXE_SVC_HTTP, NULL},
    {"sindresorhus/ky", PDXE_SVC_HTTP, NULL},
    {"phin", PDXE_SVC_HTTP, NULL},

    /* Go */
    {"net/http", PDXE_SVC_HTTP, NULL},
    {"resty", PDXE_SVC_HTTP, NULL},
    {"sling", PDXE_SVC_HTTP, NULL},
    {"heimdall", PDXE_SVC_HTTP, NULL},
    {"gentleman", PDXE_SVC_HTTP, NULL},
    {"retryablehttp", PDXE_SVC_HTTP, NULL},

    /* Java / Kotlin */
    {"HttpClient", PDXE_SVC_HTTP, NULL},
    {"OkHttp", PDXE_SVC_HTTP, NULL},
    {"okhttp3", PDXE_SVC_HTTP, NULL},
    {"RestTemplate", PDXE_SVC_HTTP, NULL},
    {"WebClient", PDXE_SVC_HTTP, NULL},
    {"Unirest", PDXE_SVC_HTTP, NULL},
    {"AsyncHttpClient", PDXE_SVC_HTTP, NULL},
    {"apache.http", PDXE_SVC_HTTP, NULL},
    {"Retrofit", PDXE_SVC_HTTP, NULL},
    {"Feign", PDXE_SVC_HTTP, NULL},
    {"ktor.client", PDXE_SVC_HTTP, NULL},
    {"kittinunf.fuel", PDXE_SVC_HTTP, NULL},

    /* Rust */
    {"reqwest", PDXE_SVC_HTTP, NULL},
    {"hyper", PDXE_SVC_HTTP, NULL},
    {"surf", PDXE_SVC_HTTP, NULL},
    {"ureq", PDXE_SVC_HTTP, NULL},
    {"isahc", PDXE_SVC_HTTP, NULL},
    {"attohttpc", PDXE_SVC_HTTP, NULL},

    /* C# */
    {"HttpClient", PDXE_SVC_HTTP, NULL},
    {"RestSharp", PDXE_SVC_HTTP, NULL},
    {"Flurl", PDXE_SVC_HTTP, NULL},
    {"Refit", PDXE_SVC_HTTP, NULL},

    /* Ruby */
    {"HTTParty", PDXE_SVC_HTTP, NULL},
    {"Faraday", PDXE_SVC_HTTP, NULL},
    {"RestClient", PDXE_SVC_HTTP, NULL},
    {"Typhoeus", PDXE_SVC_HTTP, NULL},
    {"Excon", PDXE_SVC_HTTP, NULL},
    {"Net::HTTP", PDXE_SVC_HTTP, NULL},

    /* PHP */
    {"Guzzle", PDXE_SVC_HTTP, NULL},
    {"guzzle", PDXE_SVC_HTTP, NULL},
    {"curl", PDXE_SVC_HTTP, NULL},
    {"Symfony\\HttpClient", PDXE_SVC_HTTP, NULL},

    /* C/C++ */
    {"cpr", PDXE_SVC_HTTP, NULL},
    {"cpp-httplib", PDXE_SVC_HTTP, NULL},
    {"Poco.Net", PDXE_SVC_HTTP, NULL},
    {"Beast", PDXE_SVC_HTTP, NULL},

    /* Swift */
    {"Alamofire", PDXE_SVC_HTTP, NULL},
    {"Moya", PDXE_SVC_HTTP, NULL},
    {"URLSession", PDXE_SVC_HTTP, NULL},

    /* Dart */
    {"Dio", PDXE_SVC_HTTP, NULL},
    {"dio", PDXE_SVC_HTTP, NULL},
    {"package:http", PDXE_SVC_HTTP, NULL},
    {"Chopper", PDXE_SVC_HTTP, NULL},

    /* Elixir */
    {"HTTPoison", PDXE_SVC_HTTP, NULL},
    {"Tesla", PDXE_SVC_HTTP, NULL},
    {"Finch", PDXE_SVC_HTTP, NULL},
    {"Mint.HTTP", PDXE_SVC_HTTP, NULL},

    /* Scala */
    {"sttp", PDXE_SVC_HTTP, NULL},
    {"akka.http", PDXE_SVC_HTTP, NULL},
    {"http4s", PDXE_SVC_HTTP, NULL},
    {"scalaj", PDXE_SVC_HTTP, NULL},

    /* Haskell */
    {"wreq", PDXE_SVC_HTTP, NULL},
    {"http-client", PDXE_SVC_HTTP, NULL},
    {"http-conduit", PDXE_SVC_HTTP, NULL},
    {"servant-client", PDXE_SVC_HTTP, NULL},
    {"Network.HTTP", PDXE_SVC_HTTP, NULL},

    /* Lua */
    {"socket.http", PDXE_SVC_HTTP, NULL},
    {"resty.http", PDXE_SVC_HTTP, NULL},

    /* Glued-name libraries. match_qn needs an identifier boundary around an
     * id, so a library whose own name glues a lowercase prefix/suffix onto
     * another id ("grequests", "curlpp") is listed explicitly: no boundary
     * rule can tell "grequests" from "myrequests". */
    {"grequests", PDXE_SVC_HTTP, NULL},  /* Python: gevent + requests */
    {"txrequests", PDXE_SVC_HTTP, NULL}, /* Python: Twisted + requests */
    {"redaxios", PDXE_SVC_HTTP, NULL},   /* JS: axios-compatible fetch client */
    {"gaxios", PDXE_SVC_HTTP, NULL},     /* JS: Google's axios-style client */
    {"libcurl", PDXE_SVC_HTTP, NULL},    /* C, node-libcurl */
    {"curlpp", PDXE_SVC_HTTP, NULL},     /* C++ libcurl wrapper */
    {"curlcpp", PDXE_SVC_HTTP, NULL},    /* C++ libcurl wrapper */
    {"hyperlocal", PDXE_SVC_HTTP, NULL}, /* Rust: hyper over unix sockets */
    {"guzzlehttp", PDXE_SVC_HTTP, NULL}, /* PHP: Guzzle's composer vendor */

    {NULL, PDXE_SVC_NONE, NULL},
};

/* Async dispatch / message broker libraries */
static const lib_pattern_t async_libraries[] = {
    /* GCP */
    {"cloudtasks", PDXE_SVC_ASYNC, "cloud_tasks"},
    {"cloud_tasks", PDXE_SVC_ASYNC, "cloud_tasks"},
    {"cloud.tasks", PDXE_SVC_ASYNC, "cloud_tasks"},
    {"CloudTasks", PDXE_SVC_ASYNC, "cloud_tasks"},
    {"pubsub", PDXE_SVC_ASYNC, "pubsub"},
    {"cloud.pubsub", PDXE_SVC_ASYNC, "pubsub"},
    {"PubSub", PDXE_SVC_ASYNC, "pubsub"},

    /* AWS — use SDK module paths to avoid false positives.  pdxe_fqn_compute
     * converts path slashes to '.', so a resolved local Go QN reads
     * "aws-sdk-go.service.sqs..."; include both slash and dot forms so the
     * substring match fires whether the id comes from an import path or a QN. */
    {"aws-sdk-go/service/sqs", PDXE_SVC_ASYNC, "sqs"},
    {"aws-sdk-go.service.sqs", PDXE_SVC_ASYNC, "sqs"},
    {"aws_sdk_sqs", PDXE_SVC_ASYNC, "sqs"},
    {"Amazon.SQS", PDXE_SVC_ASYNC, "sqs"},
    {"@aws-sdk/client-sqs", PDXE_SVC_ASYNC, "sqs"},
    {"boto3.client.sqs", PDXE_SVC_ASYNC, "sqs"},
    {"aws-sdk-go/service/sns", PDXE_SVC_ASYNC, "sns"},
    {"aws-sdk-go.service.sns", PDXE_SVC_ASYNC, "sns"},
    {"aws_sdk_sns", PDXE_SVC_ASYNC, "sns"},
    {"Amazon.SNS", PDXE_SVC_ASYNC, "sns"},
    {"@aws-sdk/client-sns", PDXE_SVC_ASYNC, "sns"},
    {"eventbridge", PDXE_SVC_ASYNC, "eventbridge"},
    {"EventBridge", PDXE_SVC_ASYNC, "eventbridge"},
    {"aws-sdk-go/service/lambda", PDXE_SVC_ASYNC, "lambda"},
    {"aws-sdk-go.service.lambda", PDXE_SVC_ASYNC, "lambda"},
    {"aws_sdk_lambda", PDXE_SVC_ASYNC, "lambda"},
    {"@aws-sdk/client-lambda", PDXE_SVC_ASYNC, "lambda"},
    {"stepfunctions", PDXE_SVC_ASYNC, "stepfunctions"},

    /* Azure */
    {"ServiceBus", PDXE_SVC_ASYNC, "servicebus"},
    {"Azure.Messaging", PDXE_SVC_ASYNC, "servicebus"},

    /* Kafka */
    {"kafka", PDXE_SVC_ASYNC, "kafka"},
    {"Kafka", PDXE_SVC_ASYNC, "kafka"},
    {"kafkajs", PDXE_SVC_ASYNC, "kafka"},
    {"sarama", PDXE_SVC_ASYNC, "kafka"},
    {"rdkafka", PDXE_SVC_ASYNC, "kafka"},
    {"confluent", PDXE_SVC_ASYNC, "kafka"},
    {"Confluent.Kafka", PDXE_SVC_ASYNC, "kafka"},

    /* RabbitMQ */
    {"amqp", PDXE_SVC_ASYNC, "rabbitmq"},
    {"AMQP", PDXE_SVC_ASYNC, "rabbitmq"},
    {"amqplib", PDXE_SVC_ASYNC, "rabbitmq"},
    {"RabbitMQ", PDXE_SVC_ASYNC, "rabbitmq"},
    {"lapin", PDXE_SVC_ASYNC, "rabbitmq"},
    {"MassTransit", PDXE_SVC_ASYNC, "rabbitmq"},

    /* NATS */
    {"nats", PDXE_SVC_ASYNC, "nats"},
    {"NATS", PDXE_SVC_ASYNC, "nats"},

    /* Redis pub/sub */
    {"ioredis", PDXE_SVC_ASYNC, "redis"},

    /* Task queues */
    {"celery", PDXE_SVC_ASYNC, "celery"},
    {"Celery", PDXE_SVC_ASYNC, "celery"},
    {"dramatiq", PDXE_SVC_ASYNC, "dramatiq"},
    {"huey", PDXE_SVC_ASYNC, "huey"},
    {"python-rq", PDXE_SVC_ASYNC, "rq"},
    {"rq.Queue", PDXE_SVC_ASYNC, "rq"},
    {"bullmq", PDXE_SVC_ASYNC, "bullmq"},
    {"BullMQ", PDXE_SVC_ASYNC, "bullmq"},
    {"bull.Queue", PDXE_SVC_ASYNC, "bull"},
    {"Sidekiq", PDXE_SVC_ASYNC, "sidekiq"},
    {"sidekiq", PDXE_SVC_ASYNC, "sidekiq"},
    {"Resque", PDXE_SVC_ASYNC, "resque"},
    {"GoodJob", PDXE_SVC_ASYNC, "goodjob"},
    {"DelayedJob", PDXE_SVC_ASYNC, "delayed_job"},
    {"Hangfire", PDXE_SVC_ASYNC, "hangfire"},
    {"NServiceBus", PDXE_SVC_ASYNC, "nservicebus"},
    {"asynq", PDXE_SVC_ASYNC, "asynq"},
    {"RichardKnop/machinery", PDXE_SVC_ASYNC, "machinery"},

    /* Workflow engines — use specific module paths to avoid "Temporal" in Django etc. */
    {"temporalio", PDXE_SVC_ASYNC, "temporal"},
    {"@temporalio", PDXE_SVC_ASYNC, "temporal"},
    {"temporal.client", PDXE_SVC_ASYNC, "temporal"},
    {"temporal.worker", PDXE_SVC_ASYNC, "temporal"},
    {"inngest", PDXE_SVC_ASYNC, "inngest"},

    /* Elixir */
    {"Oban", PDXE_SVC_ASYNC, "oban"},
    {"Broadway", PDXE_SVC_ASYNC, "broadway"},
    {"GenStage", PDXE_SVC_ASYNC, "genstage"},
    {"Phoenix.PubSub", PDXE_SVC_ASYNC, "phoenix_pubsub"},

    /* Scala */
    {"Alpakka", PDXE_SVC_ASYNC, "alpakka"},

    /* MQTT */
    {"mqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"paho.mqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"MQTTClient", PDXE_SVC_ASYNC, "mqtt"},
    {"mosquitto", PDXE_SVC_ASYNC, "mqtt"},
    {"asyncio_mqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"gmqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"rumqttc", PDXE_SVC_ASYNC, "mqtt"},

    /* NATS */
    {"nats.go", PDXE_SVC_ASYNC, "nats"},
    {"nats-py", PDXE_SVC_ASYNC, "nats"},
    {"nats.ws", PDXE_SVC_ASYNC, "nats"},
    {"nats.java", PDXE_SVC_ASYNC, "nats"},
    {"nats.net", PDXE_SVC_ASYNC, "nats"},
    {"async-nats", PDXE_SVC_ASYNC, "nats"},
    {"nats.rs", PDXE_SVC_ASYNC, "nats"},

    /* Dapr pub/sub */
    {"dapr.clients.grpc", PDXE_SVC_ASYNC, "dapr"},
    {"DaprClient", PDXE_SVC_ASYNC, "dapr"},

    /* Glued-name libraries (see the note in http_libraries). */
    {"aiokafka", PDXE_SVC_ASYNC, "kafka"},
    {"pykafka", PDXE_SVC_ASYNC, "kafka"},
    {"librdkafka", PDXE_SVC_ASYNC, "kafka"},
    {"rdkafkacpp", PDXE_SVC_ASYNC, "kafka"},
    {"rskafka", PDXE_SVC_ASYNC, "kafka"},
    {"aioamqp", PDXE_SVC_ASYNC, "rabbitmq"},
    {"amqpstorm", PDXE_SVC_ASYNC, "rabbitmq"},
    {"pamqp", PDXE_SVC_ASYNC, "rabbitmq"},
    {"pyamqp", PDXE_SVC_ASYNC, "rabbitmq"},
    {"amqprs", PDXE_SVC_ASYNC, "rabbitmq"},
    {"amqpcpp", PDXE_SVC_ASYNC, "rabbitmq"},
    {"pynats", PDXE_SVC_ASYNC, "nats"},
    {"jnats", PDXE_SVC_ASYNC, "nats"},
    {"aiomqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"amqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"hbmqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"umqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"mqttools", PDXE_SVC_ASYNC, "mqtt"},
    {"emqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"rumqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"gomqtt", PDXE_SVC_ASYNC, "mqtt"},
    {"libmosquitto", PDXE_SVC_ASYNC, "mqtt"},
    {"mosquittopp", PDXE_SVC_ASYNC, "mqtt"},
    {"pubsublite", PDXE_SVC_ASYNC, "pubsub"},
    {"gocelery", PDXE_SVC_ASYNC, "celery"},

    {NULL, PDXE_SVC_NONE, NULL},
};

/* Config accessor libraries */
static const lib_pattern_t config_libraries[] = {
    /* Universal */
    {"getenv", PDXE_SVC_CONFIG, NULL},
    {"Getenv", PDXE_SVC_CONFIG, NULL},
    {"getEnv", PDXE_SVC_CONFIG, NULL},
    {"LookupEnv", PDXE_SVC_CONFIG, NULL},
    {"lookupEnv", PDXE_SVC_CONFIG, NULL},
    {"get_env", PDXE_SVC_CONFIG, NULL},
    {"fetch_env", PDXE_SVC_CONFIG, NULL},
    {"GetEnvironmentVariable", PDXE_SVC_CONFIG, NULL},
    {"getProperty", PDXE_SVC_CONFIG, NULL},
    {"getEnvironment", PDXE_SVC_CONFIG, NULL},

    /* Go */
    {"viper", PDXE_SVC_CONFIG, NULL},
    {"envconfig", PDXE_SVC_CONFIG, NULL},
    {"godotenv", PDXE_SVC_CONFIG, NULL},

    /* Python */
    {"decouple", PDXE_SVC_CONFIG, NULL},
    {"dynaconf", PDXE_SVC_CONFIG, NULL},
    {"dotenv", PDXE_SVC_CONFIG, NULL},

    /* JS/TS */
    {"nconf", PDXE_SVC_CONFIG, NULL},
    {"convict", PDXE_SVC_CONFIG, NULL},
    {"envalid", PDXE_SVC_CONFIG, NULL},

    /* Rust */
    {"dotenvy", PDXE_SVC_CONFIG, NULL},
    {"figment", PDXE_SVC_CONFIG, NULL},
    {"config-rs", PDXE_SVC_CONFIG, NULL},

    /* Java/Scala */
    {"ConfigFactory", PDXE_SVC_CONFIG, NULL},
    {"ConfigurationProperties", PDXE_SVC_CONFIG, NULL},

    /* Elixir */
    {"Application.get_env", PDXE_SVC_CONFIG, NULL},
    {"Application.fetch_env", PDXE_SVC_CONFIG, NULL},

    /* Glued-name accessors (see the note in http_libraries). */
    {"wgetenv", PDXE_SVC_CONFIG, NULL},   /* Windows CRT: _wgetenv, _wgetenv_s */
    {"getenvb", PDXE_SVC_CONFIG, NULL},   /* Python: os.getenvb */
    {"qgetenv", PDXE_SVC_CONFIG, NULL},   /* Qt */
    {"dotenvx", PDXE_SVC_CONFIG, NULL},   /* JS: @dotenvx/dotenvx */
    {"phpdotenv", PDXE_SVC_CONFIG, NULL}, /* PHP: vlucas/phpdotenv */

    {NULL, PDXE_SVC_NONE, NULL},
};

/* Route registration frameworks — callee resolves to one of these AND
 * has an HTTP method suffix → PDXE_SVC_ROUTE_REG.
 * Distinguished from HTTP clients: "gin.GET" registers a handler,
 * "requests.get" makes an outbound HTTP call. */
static const lib_pattern_t route_reg_libraries[] = {
    /* Go */
    {"gin-gonic/gin", PDXE_SVC_ROUTE_REG, NULL},
    {"gin.", PDXE_SVC_ROUTE_REG, NULL},
    {"go-chi/chi", PDXE_SVC_ROUTE_REG, NULL},
    {"chi.", PDXE_SVC_ROUTE_REG, NULL},
    {"gorilla/mux", PDXE_SVC_ROUTE_REG, NULL},
    {"labstack/echo", PDXE_SVC_ROUTE_REG, NULL},
    {"echo.", PDXE_SVC_ROUTE_REG, NULL},
    {"gofiber/fiber", PDXE_SVC_ROUTE_REG, NULL},
    {"fiber.", PDXE_SVC_ROUTE_REG, NULL},
    {"net/http.ServeMux", PDXE_SVC_ROUTE_REG, NULL},
    {"http.ServeMux", PDXE_SVC_ROUTE_REG, NULL},
    {"httprouter", PDXE_SVC_ROUTE_REG, NULL},

    /* JavaScript / TypeScript */
    {"express", PDXE_SVC_ROUTE_REG, NULL},
    {"fastify", PDXE_SVC_ROUTE_REG, NULL},
    {"koa-router", PDXE_SVC_ROUTE_REG, NULL},
    {"hono", PDXE_SVC_ROUTE_REG, NULL},
    {"hapi", PDXE_SVC_ROUTE_REG, NULL},

    /* Python (non-decorator, e.g., Flask add_url_rule) */
    {"flask", PDXE_SVC_ROUTE_REG, NULL},
    {"FastAPI", PDXE_SVC_ROUTE_REG, NULL},
    {"starlette", PDXE_SVC_ROUTE_REG, NULL},

    /* PHP */
    {"Laravel", PDXE_SVC_ROUTE_REG, NULL},
    {"Illuminate.Routing", PDXE_SVC_ROUTE_REG, NULL},
    {"Symfony.Routing", PDXE_SVC_ROUTE_REG, NULL},

    /* Kotlin */
    {"ktor.server", PDXE_SVC_ROUTE_REG, NULL},
    {"ktor.routing", PDXE_SVC_ROUTE_REG, NULL},

    /* Rust */
    {"actix-web", PDXE_SVC_ROUTE_REG, NULL},
    {"actix_web", PDXE_SVC_ROUTE_REG, NULL},
    {"axum", PDXE_SVC_ROUTE_REG, NULL},
    {"rocket", PDXE_SVC_ROUTE_REG, NULL},

    /* Java */
    {"Spring", PDXE_SVC_ROUTE_REG, NULL},
    {"jakarta.ws.rs", PDXE_SVC_ROUTE_REG, NULL},

    /* C# */
    {"Microsoft.AspNetCore", PDXE_SVC_ROUTE_REG, NULL},
    {"MapGet", PDXE_SVC_ROUTE_REG, NULL},
    {"MapPost", PDXE_SVC_ROUTE_REG, NULL},

    /* Ruby */
    {"ActionDispatch", PDXE_SVC_ROUTE_REG, NULL},
    {"Sinatra", PDXE_SVC_ROUTE_REG, NULL},

    /* Elixir */
    {"Phoenix.Router", PDXE_SVC_ROUTE_REG, NULL},

    /* Scala */
    {"akka.http.scaladsl.server", PDXE_SVC_ROUTE_REG, NULL},
    {"play.api.routing", PDXE_SVC_ROUTE_REG, NULL},

    /* Glued-name frameworks (see the note in http_libraries). */
    {"apiflask", PDXE_SVC_ROUTE_REG, NULL},       /* Python: Flask-based */
    {"honox", PDXE_SVC_ROUTE_REG, NULL},          /* JS: Hono meta-framework */
    {"fasthttprouter", PDXE_SVC_ROUTE_REG, NULL}, /* Go: httprouter for fasthttp */

    {NULL, PDXE_SVC_NONE, NULL},
};

/* gRPC client libraries — protobuf stub invocations */
static const lib_pattern_t grpc_libraries[] = {
    /* Go */
    {"google.golang.org/grpc", PDXE_SVC_GRPC, NULL},
    {"grpc.Dial", PDXE_SVC_GRPC, NULL},
    {"grpc.NewClient", PDXE_SVC_GRPC, NULL},
    {"grpc.DialContext", PDXE_SVC_GRPC, NULL},

    /* Python */
    {"grpc.insecure_channel", PDXE_SVC_GRPC, NULL},
    {"grpc.secure_channel", PDXE_SVC_GRPC, NULL},
    {"grpcio", PDXE_SVC_GRPC, NULL},
    {"grpc.aio", PDXE_SVC_GRPC, NULL},

    /* Java/Kotlin */
    {"io.grpc", PDXE_SVC_GRPC, NULL},
    {"ManagedChannelBuilder", PDXE_SVC_GRPC, NULL},
    {"ManagedChannel", PDXE_SVC_GRPC, NULL},
    {"newBlockingStub", PDXE_SVC_GRPC, NULL},
    {"newFutureStub", PDXE_SVC_GRPC, NULL},

    /* C# */
    {"Grpc.Net.Client", PDXE_SVC_GRPC, NULL},
    {"GrpcChannel", PDXE_SVC_GRPC, NULL},
    {"Grpc.Core", PDXE_SVC_GRPC, NULL},

    /* JS/TS */
    {"@grpc/grpc-js", PDXE_SVC_GRPC, NULL},
    {"grpc-web", PDXE_SVC_GRPC, NULL},

    /* Rust */
    {"tonic", PDXE_SVC_GRPC, NULL},

    /* Dart/Flutter */
    {"package:grpc", PDXE_SVC_GRPC, NULL},

    {NULL, PDXE_SVC_NONE, NULL},
};

/* GraphQL client libraries */
static const lib_pattern_t graphql_libraries[] = {
    /* JS/TS */
    {"graphql-request", PDXE_SVC_GRAPHQL, NULL},
    {"@apollo/client", PDXE_SVC_GRAPHQL, NULL},
    {"apollo-client", PDXE_SVC_GRAPHQL, NULL},
    {"urql", PDXE_SVC_GRAPHQL, NULL},
    {"graphql-tag", PDXE_SVC_GRAPHQL, NULL},

    /* Python */
    {"gql", PDXE_SVC_GRAPHQL, NULL},
    {"sgqlc", PDXE_SVC_GRAPHQL, NULL},
    {"graphene", PDXE_SVC_GRAPHQL, NULL},

    /* Java */
    {"graphql-java", PDXE_SVC_GRAPHQL, NULL},
    {"DgsQueryExecutor", PDXE_SVC_GRAPHQL, NULL},

    /* Go */
    {"graphql-go", PDXE_SVC_GRAPHQL, NULL},
    {"gqlgen", PDXE_SVC_GRAPHQL, NULL},

    /* Ruby */
    {"graphql-ruby", PDXE_SVC_GRAPHQL, NULL},

    /* Rust */
    {"async-graphql", PDXE_SVC_GRAPHQL, NULL},
    {"juniper", PDXE_SVC_GRAPHQL, NULL},

    /* Glued-name libraries (see the note in http_libraries). */
    {"gqlparser", PDXE_SVC_GRAPHQL, NULL}, /* Go: vektah/gqlparser */
    {"gqlgenc", PDXE_SVC_GRAPHQL, NULL},   /* Go: gqlgen client generator */
    {"aiogqlc", PDXE_SVC_GRAPHQL, NULL},   /* Python: asyncio GraphQL client */

    {NULL, PDXE_SVC_NONE, NULL},
};

/* tRPC libraries (TypeScript only) */
static const lib_pattern_t trpc_libraries[] = {
    {"@trpc/server", PDXE_SVC_TRPC, NULL},
    {"@trpc/client", PDXE_SVC_TRPC, NULL},
    {"@trpc/react-query", PDXE_SVC_TRPC, NULL},
    {"createTRPCRouter", PDXE_SVC_TRPC, NULL},
    {"createTRPCProxyClient", PDXE_SVC_TRPC, NULL},

    {NULL, PDXE_SVC_NONE, NULL},
};

/* Method suffix type (used by both route registration and HTTP client tables) */
typedef struct {
    const char *suffix;
    const char *method;
} method_suffix_t;

/* Route registration method suffixes — matched on callee name.
 * These are methods on router objects that register handlers. */
static const method_suffix_t route_reg_suffixes[] = {
    /* HTTP method registrations */
    {".GET", "GET"},
    {".Get", "GET"},
    {".get", "GET"},
    {".POST", "POST"},
    {".Post", "POST"},
    {".post", "POST"},
    {".PUT", "PUT"},
    {".Put", "PUT"},
    {".put", "PUT"},
    {".DELETE", "DELETE"},
    {".Delete", "DELETE"},
    {".delete", "DELETE"},
    {".PATCH", "PATCH"},
    {".Patch", "PATCH"},
    {".patch", "PATCH"},
    /* Handle/HandleFunc (Go stdlib, gorilla) */
    {".Handle", "ANY"},
    {".HandleFunc", "ANY"},
    {".handle", "ANY"},
    /* Framework-specific route registration */
    {".Route", "ANY"},
    {".route", "ANY"},
    {"::get", "GET"},
    {"::post", "POST"},
    {"::put", "PUT"},
    {"::delete", "DELETE"},
    {"::patch", "PATCH"},
    /* Minimal API (C# ASP.NET) */
    {".MapGet", "GET"},
    {".MapPost", "POST"},
    {".MapPut", "PUT"},
    {".MapDelete", "DELETE"},
    /* Router mounting / prefix registration (any method) */
    {".include_router", "ANY"},
    {".mount", "ANY"},
    {".add_url_rule", "ANY"},
    {".register_blueprint", "ANY"},
    {".use", "ANY"},
    {".register", "ANY"},
    {".add_route", "ANY"},
    {".add_api_route", "ANY"},
    {".add_api_websocket_route", "ANY"},
    {NULL, NULL},
};

/* ── HTTP method inference from function/method name suffix ───── */

static const method_suffix_t method_suffixes[] = {
    {".get", "GET"},           {".Get", "GET"},           {".GET", "GET"},
    {".post", "POST"},         {".Post", "POST"},         {".POST", "POST"},
    {".put", "PUT"},           {".Put", "PUT"},           {".PUT", "PUT"},
    {".delete", "DELETE"},     {".Delete", "DELETE"},     {".DELETE", "DELETE"},
    {".patch", "PATCH"},       {".Patch", "PATCH"},       {".PATCH", "PATCH"},
    {".head", "HEAD"},         {".Head", "HEAD"},         {".HEAD", "HEAD"},
    {".options", "OPTIONS"},   {".Options", "OPTIONS"},   {"GetAsync", "GET"},
    {"PostAsync", "POST"},     {"PutAsync", "PUT"},       {"DeleteAsync", "DELETE"},
    {"SendAsync", NULL},       {"getForObject", "GET"},   {"getForEntity", "GET"},
    {"postForObject", "POST"}, {"postForEntity", "POST"}, {NULL, NULL},
};

/* ── Matching implementation ───────────────────────────────────── */

static bool qn_is_lower(char ch) {
    return ch >= 'a' && ch <= 'z';
}

static bool is_digit_char(char ch) {
    return ch >= '0' && ch <= '9';
}

static bool qn_is_alnum(char ch) {
    return qn_is_lower(ch) || is_digit_char(ch) || (ch >= 'A' && ch <= 'Z');
}

/* True when the occurrence of `id` at `hit` sits on identifier boundaries.
 *
 * A raw substring match let short ids fire inside unrelated words: "gin."
 * in "plugin.", "dio" in "studio", "surf" in "surface", "express" in
 * "expression". Every character that is not an ASCII letter or digit is a
 * separator ('.', '/', '\\', ':', '_', '-', '$', '@', ...).
 *
 *   BEFORE: start of string, a separator, an id that itself starts with a
 *           separator ("@trpc/server"), or an id that starts with an
 *           uppercase letter — a capital opens a new CamelCase word whatever
 *           precedes it ("AsyncHttpClient", "IHttpClientFactory",
 *           "NSURLSession"). Rejected: a lowercase/digit-initial id glued to
 *           a letter or digit ("plugin." / "studio" / "myrequests").
 *   AFTER:  end of string, a separator, an id that itself ends in a
 *           separator ("gin."), an uppercase letter (CamelCase glue:
 *           "GuzzleHttp", "FeignClient", "KafkaProducer", "kafkaProducer")
 *           or a digit (version suffix: "urllib2", "amqp091-go").
 *           Rejected: a lowercase letter continuing the word ("surface",
 *           "curly", "expression"). */
static bool qn_hit_on_boundary(const char *qn, const char *hit, const char *id, size_t id_len) {
    char first = id[0];
    char last = id[id_len - 1];
    if (hit > qn && qn_is_alnum(hit[-1]) && (qn_is_lower(first) || is_digit_char(first))) {
        return false;
    }
    if (qn_is_alnum(last) && qn_is_lower(hit[id_len])) {
        return false;
    }
    return true;
}

/* Check if any library identifier appears in the QN on identifier
 * boundaries (see qn_hit_on_boundary). Case-sensitive: "requests" matches
 * "project.venv.requests.api.get" and "svc.requests_get" but neither
 * "Requests" nor "myrequests". */
static const lib_pattern_t *match_qn(const char *qn, const lib_pattern_t *patterns) {
    if (!qn || !qn[0]) {
        return NULL;
    }
    for (int i = 0; patterns[i].library_id != NULL; i++) {
        const char *id = patterns[i].library_id;
        size_t id_len = strlen(id);
        if (id_len == 0) {
            continue;
        }
        for (const char *hit = strstr(qn, id); hit != NULL; hit = strstr(hit + 1, id)) {
            if (qn_hit_on_boundary(qn, hit, id, id_len)) {
                return &patterns[i];
            }
        }
    }
    return NULL;
}

static bool starts_with_segment(const char *path, const char *segment) {
    if (!path || path[0] != '/' || !segment) {
        return false;
    }
    size_t seg_len = strlen(segment);
    const char *p = path + 1;
    return strncmp(p, segment, seg_len) == 0 && (p[seg_len] == '\0' || p[seg_len] == '/');
}

static bool contains_segment(const char *path, const char *segment) {
    if (!path || !segment) {
        return false;
    }
    size_t seg_len = strlen(segment);
    const char *p = path;
    while ((p = strchr(p, '/')) != NULL) {
        p++;
        if (strncmp(p, segment, seg_len) == 0 && (p[seg_len] == '\0' || p[seg_len] == '/')) {
            return true;
        }
    }
    return false;
}

static bool has_http_route_marker(const char *path) {
    if (starts_with_segment(path, "api") || starts_with_segment(path, "apis") ||
        starts_with_segment(path, "graphql") || starts_with_segment(path, "health") ||
        starts_with_segment(path, "metrics")) {
        return true;
    }
    return path && path[0] == '/' && path[1] == 'v' && is_digit_char(path[2]) &&
           (path[3] == '\0' || path[3] == '/');
}

static bool has_filesystem_root(const char *path) {
    static const char *const roots[] = {"etc",     "root", "var",   "usr",     "home", "tmp",
                                        "private", "opt",  "bin",   "sbin",    "dev",  "proc",
                                        "sys",     "run",  "lib",   "lib64",   "mnt",  "media",
                                        "boot",    "srv",  "Users", "Volumes", NULL};
    for (int i = 0; roots[i]; i++) {
        if (starts_with_segment(path, roots[i])) {
            return true;
        }
    }
    return false;
}

static bool has_hidden_config_segment(const char *path) {
    static const char *const segments[] = {".aws", ".azure", ".config", ".docker", ".env",
                                           ".git", ".gnupg", ".kube",   ".ssh",    NULL};
    for (int i = 0; segments[i]; i++) {
        if (contains_segment(path, segments[i])) {
            return true;
        }
    }
    return false;
}

static bool path_ext_matches(const char *ext, const char *wanted) {
    return ext && wanted && strcmp(ext, wanted) == 0;
}

static bool has_filesystem_extension(const char *path) {
    if (!path) {
        return false;
    }
    const char *end = strpbrk(path, "?#");
    if (!end) {
        end = path + strlen(path);
    }
    const char *last_slash = path;
    for (const char *p = path; p < end; p++) {
        if (*p == '/') {
            last_slash = p;
        }
    }
    const char *dot = NULL;
    for (const char *p = last_slash + 1; p < end; p++) {
        if (*p == '.') {
            dot = p;
        }
    }
    if (!dot || dot == end - 1) {
        return false;
    }
    char ext[32];
    size_t ext_len = (size_t)(end - dot);
    if (ext_len >= sizeof(ext)) {
        return false;
    }
    memcpy(ext, dot, ext_len);
    ext[ext_len] = '\0';

    static const char *const hard_file_exts[] = {
        ".cfg",  ".conf",   ".credentials", ".crt",  ".db",         ".env",
        ".ini",  ".key",    ".pem",         ".pid",  ".properties", ".service",
        ".sock", ".socket", ".sqlite",      ".toml", NULL};
    for (int i = 0; hard_file_exts[i]; i++) {
        if (path_ext_matches(ext, hard_file_exts[i])) {
            return true;
        }
    }
    if ((path_ext_matches(ext, ".json") || path_ext_matches(ext, ".yaml") ||
         path_ext_matches(ext, ".yml") || path_ext_matches(ext, ".xml")) &&
        !has_http_route_marker(path)) {
        return true;
    }
    return false;
}

static bool callee_is_delimiter_or_filesystem_builder(const char *callee_name) {
    if (!callee_name) {
        return false;
    }
    const char *last_dot = strrchr(callee_name, '.');
    const char *last_colon = strstr(callee_name, "::");
    const char *method = callee_name;
    if (last_dot && last_dot[1]) {
        method = last_dot + 1;
    }
    if (last_colon && last_colon[2]) {
        method = last_colon + 2;
    }
    if (strcmp(method, "split") == 0 || strcmp(method, "rsplit") == 0 ||
        strcmp(method, "partition") == 0 || strcmp(method, "join") == 0 ||
        strcmp(method, "replace") == 0 || strcmp(method, "replaceAll") == 0 ||
        strcmp(method, "match") == 0 || strcmp(method, "matchAll") == 0 ||
        strcmp(method, "search") == 0 || strcmp(method, "test") == 0 ||
        strcmp(method, "exec") == 0) {
        return true;
    }
    return strstr(callee_name, "os.path.join") != NULL || strstr(callee_name, "path.join") != NULL;
}

static const char *strip_string_delimiters(const char *literal, char *buf, size_t buf_sz) {
    if (!literal || !literal[0]) {
        return NULL;
    }
    const char *start = literal;
    while (*start == ' ' || *start == '\t' || *start == '\n' || *start == '\r') {
        start++;
    }
    size_t len = strlen(start);
    while (len > 0 && (start[len - 1] == ' ' || start[len - 1] == '\t' || start[len - 1] == '\n' ||
                       start[len - 1] == '\r')) {
        len--;
    }
    if (len >= 2 && (start[0] == '"' || start[0] == '\'' || start[0] == '`') &&
        start[len - 1] == start[0]) {
        start++;
        len -= 2;
    }
    if (len == 0 || len >= buf_sz) {
        return NULL;
    }
    memcpy(buf, start, len);
    buf[len] = '\0';
    return buf;
}

/* A comment opens with the slash a route opens with, and an argument list
 * that starts with one handed three Java block comments to the Route pass as
 * URLs (elasticsearch, 2026-09-16). The wildcard route (slash-star alone) is a
 * path, so the block-comment shape needs its closing star-slash; a route
 * literal never holds a line break. */
bool pdxe_service_pattern_is_comment_text(const char *text) {
    if (!text || text[0] != '/') {
        return false;
    }
    if (strchr(text, '\n') != NULL || strchr(text, '\r') != NULL) {
        return true;
    }
    size_t n = strlen(text);
    if (text[1] == '*' && n >= 4 && text[n - 2] == '*' && text[n - 1] == '/') {
        return true;
    }
    return text[1] == '/' && (text[2] == ' ' || text[2] == '\t');
}

bool pdxe_service_pattern_is_http_route_literal(const char *literal, const char *callee_name) {
    char path_buf[1024];
    const char *path = strip_string_delimiters(literal, path_buf, sizeof(path_buf));
    if (!path || !path[0]) {
        return false;
    }
    if (strncmp(path, "http://", 7) == 0 || strncmp(path, "https://", 8) == 0) {
        return true;
    }
    if (strstr(path, "://") != NULL) {
        return false;
    }
    if (path[0] != '/') {
        return false;
    }
    if (pdxe_service_pattern_is_comment_text(path)) {
        return false;
    }
    if (callee_is_delimiter_or_filesystem_builder(callee_name)) {
        return false;
    }
    if (has_filesystem_root(path) || has_hidden_config_segment(path) ||
        has_filesystem_extension(path)) {
        return false;
    }
    return true;
}

/* ── Public API ────────────────────────────────────────────────── */

/* Per-worker TLS cache of pdxe_service_pattern_match results.
 * The hot path in resolve_file_calls invokes pattern matching for
 * EVERY resolved CALL (via emit_service_edge) — that's 6 pattern-list
 * scans × ~30 patterns × strstr per call. On kubernetes (~600k
 * resolved call edges), the same resolved QN (e.g. "context.Context.
 * Done", "fmt.Errorf", "errors.New") repeats hundreds of thousands of
 * times. A simple TLS hash cache turns the linear scan into one
 * lookup after the first miss for that QN. Lifetime is per-worker for
 * the duration of the parallel_resolve phase. */
#include "foundation/hash_table.h"
#include "foundation/compat.h"

static PDXE_TLS PDXEHashTable *_svc_cache = NULL;
/* Encode the enum + 1 in the pointer so 0/NULL means "miss". */
static inline void *svc_enum_to_ptr(pdxe_svc_kind_t k) {
    return (void *)(uintptr_t)((unsigned)k + 1u);
}
static inline pdxe_svc_kind_t svc_ptr_to_enum(void *p) {
    return (pdxe_svc_kind_t)((uintptr_t)p - 1u);
}

static void svc_cache_free_key(const char *key, void *val, void *ud) {
    (void)val;
    (void)ud;
    free((char *)key);
}

void pdxe_service_pattern_cache_begin(void) {
    if (_svc_cache)
        return; /* idempotent */
    _svc_cache = pdxe_ht_create(8192);
}

void pdxe_service_pattern_cache_end(void) {
    if (!_svc_cache)
        return;
    pdxe_ht_foreach(_svc_cache, svc_cache_free_key, NULL);
    pdxe_ht_free(_svc_cache);
    _svc_cache = NULL;
}

void pdxe_service_patterns_init(void) {
    /* No-op — tables are static const */
}

bool pdxe_service_pattern_is_global_fetch(const char *callee_name) {
    return callee_name != NULL && strcmp(callee_name, "fetch") == 0;
}

pdxe_svc_kind_t pdxe_service_pattern_match(const char *resolved_qn) {
    if (!resolved_qn || !resolved_qn[0]) {
        return PDXE_SVC_NONE;
    }

    if (_svc_cache) {
        void *cached = pdxe_ht_get(_svc_cache, resolved_qn);
        if (cached) {
            return svc_ptr_to_enum(cached);
        }
    }

    pdxe_svc_kind_t result = PDXE_SVC_NONE;
    const lib_pattern_t *p;

    /* Route registration checked first — prevents gin/echo from matching
     * as HTTP clients (both have .get/.post suffixes). */
    if ((p = match_qn(resolved_qn, route_reg_libraries)))
        result = p->kind;
    else if ((p = match_qn(resolved_qn, http_libraries)))
        result = p->kind;
    else if ((p = match_qn(resolved_qn, async_libraries)))
        result = p->kind;
    else if ((p = match_qn(resolved_qn, config_libraries)))
        result = p->kind;
    else if ((p = match_qn(resolved_qn, grpc_libraries)))
        result = p->kind;
    else if ((p = match_qn(resolved_qn, graphql_libraries)))
        result = p->kind;
    else if ((p = match_qn(resolved_qn, trpc_libraries)))
        result = p->kind;

    if (_svc_cache) {
        char *kdup = strdup(resolved_qn);
        if (kdup)
            pdxe_ht_set(_svc_cache, kdup, svc_enum_to_ptr(result));
    }
    return result;
}

const char *pdxe_service_pattern_http_method(const char *callee_name) {
    if (!callee_name) {
        return NULL;
    }
    for (int i = 0; method_suffixes[i].suffix != NULL; i++) {
        size_t slen = strlen(method_suffixes[i].suffix);
        size_t clen = strlen(callee_name);
        if (clen >= slen && strcmp(callee_name + clen - slen, method_suffixes[i].suffix) == 0) {
            return method_suffixes[i].method;
        }
    }
    return NULL;
}

const char *pdxe_service_pattern_route_method(const char *callee_name) {
    if (!callee_name) {
        return NULL;
    }
    size_t clen = strlen(callee_name);
    for (int i = 0; route_reg_suffixes[i].suffix != NULL; i++) {
        size_t slen = strlen(route_reg_suffixes[i].suffix);
        if (clen >= slen && strcmp(callee_name + clen - slen, route_reg_suffixes[i].suffix) == 0) {
            return route_reg_suffixes[i].method;
        }
    }
    return NULL;
}

const char *pdxe_service_pattern_broker(const char *resolved_qn) {
    if (!resolved_qn) {
        return NULL;
    }
    const lib_pattern_t *p = match_qn(resolved_qn, async_libraries);
    return p ? p->broker : NULL;
}
