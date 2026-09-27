FROM python:3.12-slim

# Build-time system packages. psycopg2 needs libpq and a compiler, the
# embedding stack needs a toolchain, openssl generates the dev certificate
# below, and git is used at run time to clone the repositories being indexed.
RUN apt-get update && apt-get install -y build-essential libpq-dev openssl git && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Fetch the embedding model now, into the image. Left until first request it
# would make a cold container hang for a minute on a download, and would
# require outbound network from wherever this eventually runs.
RUN python -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('Alibaba-NLP/gte-base-en-v1.5', trust_remote_code=True)"

# A self-signed certificate so the service can speak TLS out of the box.
# Mount real certificates over this path for anything beyond local use --
# no client will trust this one.
RUN mkdir -p /app/certs && \
    openssl req -x509 -nodes -days 3650 -newkey rsa:2048 \
      -keyout /app/certs/selfsigned.key \
      -out /app/certs/selfsigned.crt \
      -subj "/C=XX/ST=Dev/L=Dev/O=Dev/CN=localhost"

COPY app/ ./app/
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

EXPOSE 8000

ENTRYPOINT ["/app/docker-entrypoint.sh"]
