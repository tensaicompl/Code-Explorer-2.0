FROM alpine:3.20
RUN adduser -D app
WORKDIR /home/app
COPY --chown=app:app entrypoint.sh /usr/local/bin/
SHELL ["/bin/sh", "-c"]
ONBUILD RUN echo "child build"
STOPSIGNAL SIGTERM
VOLUME ["/data"]
ENTRYPOINT ["entrypoint.sh"]
