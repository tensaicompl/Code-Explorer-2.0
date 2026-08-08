#!/bin/sh
SSL_CERTFILE="${SSL_CERTFILE:-/app/certs/selfsigned.crt}"
SSL_KEYFILE="${SSL_KEYFILE:-/app/certs/selfsigned.key}"
exec uvicorn app.main:app --host 0.0.0.0 --port 8000 \
    --workers "${UVICORN_WORKERS:-2}" \
    --ssl-certfile "$SSL_CERTFILE" \
    --ssl-keyfile "$SSL_KEYFILE"
