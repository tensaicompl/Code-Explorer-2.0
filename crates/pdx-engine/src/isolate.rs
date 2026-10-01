//! Extraction in a child process, so that the engine crashing costs one file, not the
//! indexing run.
//!
//! With `PDX_ENGINE_ISOLATE=1` an [`Extractor`] hands batches of files to a worker: the
//! same program started with the hidden `engine-worker` subcommand, which serves
//! extraction over its standard input and output. If the engine aborts, only the
//! worker dies. The parent sees the worker's output end early, discards everything the
//! batch produced, starts a new worker, and extracts the batch's files again one at a
//! time; a file that kills the worker on its own is reported as failed with reason
//! `engine_crash`, and every other file is extracted. So the outcome for each file is
//! the same however files were batched, and the same as extracting it in process.
//!
//! The protocol is a sequence of frames, each a little-endian `u64` length and that
//! many bytes of postcard. The worker speaks first, naming the protocol and the engine
//! version, and the parent refuses a worker that does not match it. Then each request
//! is one batch and each response one outcome per file, in order. A frame longer than
//! [`MAX_FRAME_BYTES`], one that ends early, one that does not decode exactly, or a
//! response with the wrong number of outcomes is never accepted, in whole or in part.
//!
//! Workers are ordinary child processes on every system; nothing here forks.

use std::ffi::OsString;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::error::EngineError;
use crate::model::{FileExtract, serde_bytes_compat};

/// The environment variable that asks for isolation: set to `1`.
pub const ISOLATE_ENV: &str = "PDX_ENGINE_ISOLATE";

/// The hidden subcommand that makes a program serve as a worker.
pub const WORKER_SUBCOMMAND: &str = "engine-worker";

/// The largest frame either side accepts. A frame is one batch of sources, or one
/// batch's extractions; four gibibytes is beyond any batch the memory budget allows,
/// and small enough that a corrupt length cannot ask for an absurd allocation.
pub const MAX_FRAME_BYTES: u64 = 4 << 30;

/// Version of the protocol below; a worker of another version is refused.
const PROTOCOL: u32 = 1;

/// Whether the environment asks for isolation.
pub fn isolation_requested() -> bool {
    std::env::var_os(ISOLATE_ENV).is_some_and(|v| v == "1")
}

/// A file to extract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    /// Its language, by the language matrix's identifier.
    pub language: String,
    /// Its path relative to the repository.
    pub rel_path: String,
    /// Its source.
    #[serde(with = "serde_bytes_compat")]
    pub source: Vec<u8>,
}

/// Why a file has no extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ExtractFailure {
    /// The engine crashed on the file.
    #[error("the engine crashed on the file")]
    EngineCrash,
    /// The engine refused or failed to extract it.
    #[error(transparent)]
    Engine(#[from] EngineError),
}

impl ExtractFailure {
    /// The reason a file is recorded as failed with.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::EngineCrash => "engine_crash",
            Self::Engine(_) => "engine_error",
        }
    }
}

/// What happened to one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractOutcome {
    /// It was extracted.
    Extracted(Box<FileExtract>),
    /// It was not.
    Failed(ExtractFailure),
}

impl From<Result<FileExtract, EngineError>> for ExtractOutcome {
    fn from(result: Result<FileExtract, EngineError>) -> Self {
        match result {
            Ok(extract) => Self::Extracted(Box::new(extract)),
            Err(error) => Self::Failed(ExtractFailure::Engine(error)),
        }
    }
}

/// Isolation could not be had at all: no worker could be started, or the one started
/// is not a worker this program can talk to.
#[derive(Debug, thiserror::Error)]
pub enum IsolationError {
    /// The worker program could not be started.
    #[error("cannot start the engine worker: {0}")]
    Spawn(#[source] io::Error),
    /// The worker did not introduce itself as a worker of this protocol and engine.
    #[error("the engine worker did not start: {0}")]
    Handshake(String),
}

/// How to start a worker: a program, its arguments and extra environment.
#[derive(Debug, Clone)]
pub struct WorkerCommand {
    program: PathBuf,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
}

impl WorkerCommand {
    /// `program`, with no arguments yet.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
        }
    }

    /// This program, as a worker: the running executable with the worker subcommand.
    ///
    /// # Errors
    ///
    /// When the running executable cannot be found.
    pub fn current_exe() -> io::Result<Self> {
        Ok(Self::new(std::env::current_exe()?).arg(WORKER_SUBCOMMAND))
    }

    /// Adds an argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Sets an environment variable for the worker only.
    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }
}

/// Extraction in process, or isolated in workers, as the environment asks.
pub enum Extractor {
    /// In this process.
    InProcess(Engine),
    /// In a worker process.
    Isolated(IsolatedExtractor),
}

impl Extractor {
    /// Isolated when [`ISOLATE_ENV`] is `1`, with workers started by `worker`; in
    /// process otherwise.
    ///
    /// # Errors
    ///
    /// The engine's failure to start, in process.
    pub fn from_env(worker: WorkerCommand) -> Result<Self, EngineError> {
        if isolation_requested() {
            Ok(Self::Isolated(IsolatedExtractor::new(worker)))
        } else {
            Ok(Self::InProcess(Engine::new()?))
        }
    }

    /// Extracts a batch: one outcome per file, in order.
    ///
    /// # Errors
    ///
    /// Isolated, when no worker can be started at all.
    pub fn extract_batch(
        &mut self,
        files: &[SourceFile],
    ) -> Result<Vec<ExtractOutcome>, IsolationError> {
        match self {
            Self::InProcess(engine) => Ok(files
                .iter()
                .map(|f| engine.extract(&f.language, &f.rel_path, &f.source).into())
                .collect()),
            Self::Isolated(isolated) => isolated.extract_batch(files),
        }
    }
}

/// Extraction in worker processes, restarted after every crash.
pub struct IsolatedExtractor {
    command: WorkerCommand,
    worker: Option<Worker>,
    crashes: u64,
}

impl IsolatedExtractor {
    /// Starts workers with `command`, the first when there is something to extract.
    pub fn new(command: WorkerCommand) -> Self {
        Self {
            command,
            worker: None,
            crashes: 0,
        }
    }

    /// How many times a worker has died under this extractor.
    pub fn crashes(&self) -> u64 {
        self.crashes
    }

    /// Extracts a batch in a worker: one outcome per file, in order. A worker that dies
    /// costs the files that kill it, never the others.
    ///
    /// # Errors
    ///
    /// When no worker can be started at all.
    pub fn extract_batch(
        &mut self,
        files: &[SourceFile],
    ) -> Result<Vec<ExtractOutcome>, IsolationError> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(outcomes) = self.attempt(files)? {
            return Ok(outcomes);
        }
        if let [_] = files {
            return Ok(vec![ExtractOutcome::Failed(ExtractFailure::EngineCrash)]);
        }
        // Nothing the dead worker sent for this batch is used. Each file is extracted
        // again on its own, so the crash is pinned on the files that cause it.
        let mut outcomes = Vec::with_capacity(files.len());
        for file in files {
            outcomes.extend(self.extract_batch(std::slice::from_ref(file))?);
        }
        Ok(outcomes)
    }

    /// One exchange with a worker: the outcomes, or `None` if the worker died.
    fn attempt(
        &mut self,
        files: &[SourceFile],
    ) -> Result<Option<Vec<ExtractOutcome>>, IsolationError> {
        if self.worker.is_none() {
            self.worker = Some(Worker::start(&self.command)?);
        }
        let worker = self.worker.as_mut().expect("a worker was just started");
        if let Ok(outcomes) = worker.exchange(files) {
            return Ok(Some(outcomes));
        }
        // Dropping the worker kills and reaps it.
        self.worker = None;
        self.crashes += 1;
        Ok(None)
    }
}

/// What the parent sends: a batch, borrowed, encoded exactly as [`Request`] decodes.
#[derive(Serialize)]
enum RequestRef<'a> {
    Extract(&'a [SourceFile]),
}

/// What the worker receives.
#[derive(Deserialize)]
enum Request {
    Extract(Vec<SourceFile>),
}

/// What the worker sends.
#[derive(Serialize, Deserialize)]
enum Response {
    /// Its first frame.
    Ready {
        protocol: u32,
        engine_version: String,
    },
    /// One outcome per file of the batch, in order.
    Extracted(Vec<Result<FileExtract, EngineError>>),
}

/// A running worker. Dropping it kills and reaps the process.
struct Worker {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Worker {
    fn start(command: &WorkerCommand) -> Result<Self, IsolationError> {
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .envs(command.envs.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The parent's own streams may carry a protocol; a dying worker writes
            // nothing to them.
            .stderr(Stdio::null())
            .spawn()
            .map_err(IsolationError::Spawn)?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(IsolationError::Handshake("no pipes to the worker".into()));
        };
        let mut worker = Self {
            child,
            input: BufWriter::new(input),
            output: BufReader::new(output),
        };
        match read_frame::<Response>(&mut worker.output) {
            Ok(Some(Response::Ready {
                protocol,
                engine_version,
            })) => {
                let ours = Engine::version();
                if protocol != PROTOCOL || engine_version != ours {
                    return Err(IsolationError::Handshake(format!(
                        "it speaks protocol {protocol} for engine {engine_version}, \
                         not protocol {PROTOCOL} for engine {ours}"
                    )));
                }
                Ok(worker)
            }
            Ok(_) => Err(IsolationError::Handshake(
                "it did not introduce itself".into(),
            )),
            Err(e) => Err(IsolationError::Handshake(e.to_string())),
        }
    }

    fn exchange(&mut self, files: &[SourceFile]) -> Result<Vec<ExtractOutcome>, FrameError> {
        write_frame(&mut self.input, &RequestRef::Extract(files))?;
        self.input.flush()?;
        match read_frame::<Response>(&mut self.output)? {
            Some(Response::Extracted(outcomes)) if outcomes.len() == files.len() => {
                Ok(outcomes.into_iter().map(ExtractOutcome::from).collect())
            }
            Some(_) => Err(FrameError::Malformed(
                "a response that does not answer the batch".into(),
            )),
            None => Err(FrameError::Truncated),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // A worker holds nothing worth finishing; it is stopped without ceremony, and
        // reaped so no process is left behind.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Serves extraction on standard input and output until the input ends: the worker's
/// side of the protocol, for the hidden `engine-worker` subcommand.
///
/// # Errors
///
/// When the engine cannot start, or the streams fail or carry something that is not
/// the protocol.
pub fn serve_worker() -> io::Result<()> {
    quiet_crashes();
    serve(io::stdin().lock(), io::stdout().lock())
}

/// The worker's loop, over any pair of streams.
fn serve(input: impl Read, output: impl Write) -> io::Result<()> {
    let mut input = BufReader::new(input);
    let mut output = BufWriter::new(output);
    let engine = Engine::new().map_err(io::Error::other)?;
    write_frame(
        &mut output,
        &Response::Ready {
            protocol: PROTOCOL,
            engine_version: Engine::version(),
        },
    )
    .map_err(io::Error::other)?;
    output.flush()?;
    while let Some(Request::Extract(files)) =
        read_frame::<Request>(&mut input).map_err(io::Error::other)?
    {
        let outcomes = files
            .iter()
            .map(|f| engine.extract(&f.language, &f.rel_path, &f.source))
            .collect();
        write_frame(&mut output, &Response::Extracted(outcomes)).map_err(io::Error::other)?;
        output.flush()?;
    }
    Ok(())
}

/// On Windows, a crashing process can stop at a dialog that waits for someone to
/// dismiss it, which would leave the parent waiting too. A worker dies quietly instead.
#[cfg(windows)]
fn quiet_crashes() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    // SAFETY: SetErrorMode only sets this process's error mode.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    }
}

#[cfg(not(windows))]
fn quiet_crashes() {}

/// Why a frame was not accepted.
#[derive(Debug, thiserror::Error)]
enum FrameError {
    #[error("the stream ended inside a frame")]
    Truncated,
    #[error("a frame of {0} bytes exceeds the limit")]
    TooLarge(u64),
    #[error("a frame does not decode: {0}")]
    Malformed(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn write_frame<T: Serialize>(w: &mut impl Write, value: &T) -> Result<(), FrameError> {
    let payload = postcard::to_stdvec(value).map_err(|e| FrameError::Malformed(e.to_string()))?;
    let len = payload.len() as u64;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&payload)?;
    Ok(())
}

/// The next frame, or `None` if the stream ends cleanly before one starts.
fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> Result<Option<T>, FrameError> {
    let mut len = [0u8; 8];
    let mut got = 0;
    while got < len.len() {
        match r.read(&mut len[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let len = u64::from_le_bytes(len);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    // Read as the bytes arrive rather than allocated up front, so a length that
    // promises more than the stream holds costs only what the stream holds.
    let mut payload = Vec::new();
    r.take(len).read_to_end(&mut payload)?;
    if payload.len() as u64 != len {
        return Err(FrameError::Truncated);
    }
    let (value, rest) =
        postcard::take_from_bytes(&payload).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if !rest.is_empty() {
        return Err(FrameError::Malformed(format!(
            "{} bytes after the value",
            rest.len()
        )));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame<T: Serialize>(value: &T) -> Vec<u8> {
        let mut out = Vec::new();
        write_frame(&mut out, value).expect("an in-memory frame");
        out
    }

    #[test]
    fn a_frame_round_trips() {
        let bytes = frame(&(7u32, "seven".to_owned()));
        let back: (u32, String) = read_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert_eq!(back, (7, "seven".to_owned()));
    }

    #[test]
    fn a_clean_end_is_no_frame() {
        assert!(read_frame::<u32>(&mut [].as_slice()).unwrap().is_none());
    }

    #[test]
    fn nothing_of_a_partial_frame_is_accepted() {
        let bytes = frame(&vec![1u64, 2, 3, 4, 5]);
        for cut in 1..bytes.len() {
            let result = read_frame::<Vec<u64>>(&mut &bytes[..cut]);
            assert!(
                matches!(result, Err(FrameError::Truncated)),
                "a frame cut at {cut} of {} bytes was {result:?}",
                bytes.len()
            );
        }
    }

    #[test]
    fn an_oversized_length_is_refused_before_reading() {
        let mut bytes = (MAX_FRAME_BYTES + 1).to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; 16]);
        assert!(matches!(
            read_frame::<u32>(&mut bytes.as_slice()),
            Err(FrameError::TooLarge(_))
        ));
    }

    #[test]
    fn a_frame_must_decode_exactly() {
        // A value followed by bytes it does not account for.
        let mut payload = postcard::to_stdvec(&5u32).unwrap();
        payload.push(0);
        let mut bytes = (payload.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(&payload);
        assert!(matches!(
            read_frame::<u32>(&mut bytes.as_slice()),
            Err(FrameError::Malformed(_))
        ));
        // Bytes that are no value of the type at all.
        let bytes = frame(&"not a number".to_owned());
        assert!(matches!(
            read_frame::<Vec<FileExtract>>(&mut bytes.as_slice()),
            Err(FrameError::Malformed(_))
        ));
    }

    #[test]
    fn a_worker_serves_what_the_engine_extracts() {
        let files = vec![
            SourceFile {
                language: "python".into(),
                rel_path: "a.py".into(),
                source: b"def f():\n    return g()\n".to_vec(),
            },
            SourceFile {
                language: "no-such-language".into(),
                rel_path: "b.txt".into(),
                source: b"x".to_vec(),
            },
        ];
        let input = frame(&RequestRef::Extract(&files));
        let mut output = Vec::new();
        // The worker's engine is started and dropped inside `serve`, so the one below
        // runs on this thread only after it is gone.
        serve(input.as_slice(), &mut output).expect("the worker serves");

        let mut stream = output.as_slice();
        let Some(Response::Ready { protocol, .. }) = read_frame(&mut stream).unwrap() else {
            panic!("the worker did not introduce itself");
        };
        assert_eq!(protocol, PROTOCOL);
        let Some(Response::Extracted(outcomes)) = read_frame(&mut stream).unwrap() else {
            panic!("the worker did not answer");
        };
        assert!(read_frame::<Response>(&mut stream).unwrap().is_none());

        let engine = Engine::new().unwrap();
        let direct: Vec<_> = files
            .iter()
            .map(|f| engine.extract(&f.language, &f.rel_path, &f.source))
            .collect();
        assert_eq!(outcomes, direct);
        assert_eq!(
            outcomes[1],
            Err(EngineError::UnknownLanguage("no-such-language".into()))
        );
    }
}
