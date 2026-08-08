FROM python:3.12-slim

# tree-sitter grammars compile from C at install time, so the image needs a
# toolchain even though nothing here is written in C.
RUN apt-get update && apt-get install -y build-essential && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# CPU-only torch, ahead of anything that would otherwise pull the CUDA build
# and its couple of gigabytes of driver libraries.
RUN pip install --no-cache-dir torch --index-url https://download.pytorch.org/whl/cpu

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Fetch the model into the image. The indexer forks per project, and without
# this each child would try to download it independently on first use.
RUN python -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('Alibaba-NLP/gte-base-en-v1.5', trust_remote_code=True)"

COPY . .

CMD ["python", "build_corpus.py"]
