"""Every knob the corpus builder reads from the environment, in one place.

Separated out so the builder, the chunker, the extractors and the persistence
layer can all share these without importing each other -- which, given the
builder imports all three, would be a cycle.
"""
import os

DATABASE_URL = os.environ.get("DATABASE_URL", "postgresql://prx:prx@postgres:5432/prx")
CODEBASE_DIR = os.environ.get("CODEBASE_DIR", "/data/codebase")
EMBEDDING_MODEL = os.environ.get("EMBEDDING_MODEL", "Alibaba-NLP/gte-base-en-v1.5")

#: Characters per chunk, and how much neighbouring chunks share. The overlap is
#: what stops a declaration and its body landing in different chunks and
#: matching neither.
CHUNK_SIZE = 800
CHUNK_OVERLAP = 200

#: How many chunks are embedded per model call, and per database round trip.
EMBED_BATCH_SIZE = int(os.environ.get("EMBED_BATCH_SIZE", "128"))
DB_BATCH_SIZE = 500

#: Data rather than code. Large ones are skipped: a megabyte of generated JSON
#: costs as much to embed as a megabyte of source and answers nothing.
DATA_EXTENSIONS = {'.json', '.yaml', '.yml', '.properties', '.gradle', '.gpr', '.xml', '.sql'}
MAX_DATA_FILE_SIZE = int(os.environ.get("MAX_DATA_FILE_SIZE", str(1 * 1024 * 1024)))
MAX_SOURCE_FILE_SIZE = int(os.environ.get("MAX_SOURCE_FILE_SIZE", str(5 * 1024 * 1024)))

#: Never descended into.
EXCLUDED_DIRS = {"scripts", "__pycache__", ".git", "node_modules", "target", "build"}

#: What the walk will open at all. Adding an extractor without adding its
#: extension here does nothing, silently -- the file is never read.
SOURCE_EXTENSIONS = {
    ".ads", ".adb", ".ada",
    ".c", ".h", ".hh", ".cc",
    ".py",
    ".pl", ".pm",
    ".sh", ".bash", ".ksh",
    ".cpp", ".hpp",
    ".rb",
    ".idl",
    ".incl",
    ".sql",
    ".gpr",
    ".json", ".yaml", ".yml",
    ".java", ".kt", ".scala", ".ts", ".js", ".tsx", ".jsx",
    ".rs",
    ".properties", ".gradle",
    ".proto", ".md", ".env",
}
