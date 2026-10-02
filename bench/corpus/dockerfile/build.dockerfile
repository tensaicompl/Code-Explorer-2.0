# syntax=docker/dockerfile:1
ARG BASE=debian:bookworm-slim
FROM ${BASE} AS build
ARG VERSION=1.0
ENV LANG=C.UTF-8 \
    APP_HOME=/opt/app
WORKDIR ${APP_HOME}
COPY . .
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends make gcc; \
    make build VERSION="${VERSION}"; \
    rm -rf /var/lib/apt/lists/*

FROM ${BASE} AS runtime
LABEL org.opencontainers.image.title="corpus" description="multi-stage \"build\""
COPY --from=build /opt/app/bin/ /usr/local/bin/
USER 1000:1000
EXPOSE 8080/tcp
HEALTHCHECK --interval=30s --timeout=3s CMD ["/usr/local/bin/app", "--health"]
ENTRYPOINT ["/usr/local/bin/app"]
CMD ["--serve"]
