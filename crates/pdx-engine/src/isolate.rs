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
//! Every exchange with a worker, its introduction included, is bounded in time: the
//! request written and the response read must both be done within the extractor's
//! timeout ([`DEFAULT_EXCHANGE_TIMEOUT`] unless constructed with another). A worker that
//! takes longer is stopped and reaped, whatever it sent is discarded, and the batch
//! fails with [`IsolationError::Timeout`]. A timeout is not a crash: nothing is retried
//! and no file is recorded as `engine_crash`, because a file that hangs the engine
//! would hang every retry too. The caller treats it as fatal to the build.
//!
//! Workers are ordinary child processes on every system; nothing here forks. A
//! worker starts no processes of its own, so stopping it closes its end of every
//! pipe, which is what ends the parent's thread talking to it.

use std::ffi::OsString;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

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

/// How long one exchange with a worker may take, its request written and its response
/// read, before the worker is stopped: two minutes, far beyond any batch the engine
/// extracts without hanging.
pub const DEFAULT_EXCHANGE_TIMEOUT: Duration = Duration::from_secs(120);

/// Version of the protocol below; a worker of another version is refused. 2 since
/// `FileExtract` gained `extraction_lost`, 3 since a definition carries its base
/// classes and an extraction the namespace its file declares, 4 since an extraction
/// carries its `impl Trait for Type` relations (issue 42), 5 since a call carries its
/// node-type path and arguments and a definition its decorators, parameter types and
/// route (issues 46 and 47), 6 since a definition carries every route binding with its
/// declaring node in place of one route (issue 54).
const PROTOCOL: u32 = 6;

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

/// Isolation failed as a whole: no worker could be started, the one started is not a
/// worker this program can talk to, or a worker did not answer in time.
#[derive(Debug, thiserror::Error)]
pub enum IsolationError {
    /// The worker program could not be started.
    #[error("cannot start the engine worker: {0}")]
    Spawn(#[source] io::Error),
    /// The worker did not introduce itself as a worker of this protocol and engine.
    #[error("the engine worker did not start: {0}")]
    Handshake(String),
    /// The worker did not finish an exchange within the timeout. It has been stopped
    /// and reaped, and nothing it sent was used.
    #[error("the engine worker (process {worker}) did not answer within {after:?}; it was stopped")]
    Timeout {
        /// The timeout it exceeded.
        after: Duration,
        /// Its process id, which no longer names a running process of this program.
        worker: u32,
    },
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
    /// Isolated when [`ISOLATE_ENV`] is `1`, with workers started by `worker` and the
    /// default timeout; in process otherwise.
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
    /// Isolated, when no worker can be started at all, or one does not answer in time.
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
    timeout: Duration,
    worker: Option<Worker>,
    crashes: u64,
}

impl IsolatedExtractor {
    /// Starts workers with `command`, the first when there is something to extract,
    /// with the default timeout.
    pub fn new(command: WorkerCommand) -> Self {
        Self::with_timeout(command, DEFAULT_EXCHANGE_TIMEOUT)
    }

    /// Starts workers with `command`, stopping one that takes longer than `timeout`
    /// over an exchange.
    pub fn with_timeout(command: WorkerCommand, timeout: Duration) -> Self {
        Self {
            command,
            timeout,
            worker: None,
            crashes: 0,
        }
    }

    /// How many times a worker has died under this extractor. A worker stopped for
    /// taking too long did not die, and is not counted.
    pub fn crashes(&self) -> u64 {
        self.crashes
    }

    /// The running worker's process id, if one is running.
    pub fn worker_id(&self) -> Option<u32> {
        self.worker.as_ref().map(|w| w.child.id())
    }

    /// Extracts a batch in a worker: one outcome per file, in order. A worker that dies
    /// costs the files that kill it, never the others.
    ///
    /// # Errors
    ///
    /// When no worker can be started at all, or one does not answer in time.
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
            self.worker = Some(Worker::start(&self.command, self.timeout)?);
        }
        let worker = self.worker.as_mut().expect("a worker was just started");
        match worker.exchange(files, self.timeout) {
            Ok(outcomes) => Ok(Some(outcomes)),
            Err(Exchange::TimedOut) => {
                let worker = self.worker.take().expect("the worker that timed out");
                Err(IsolationError::Timeout {
                    after: self.timeout,
                    worker: worker.child.id(),
                })
            }
            Err(Exchange::Failed(_)) => {
                // Dropping the worker kills and reaps it.
                self.worker = None;
                self.crashes += 1;
                Ok(None)
            }
        }
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

/// Why an exchange with a worker produced nothing.
#[derive(Debug)]
enum Exchange {
    /// The worker did not finish it in time, and has been stopped and reaped.
    TimedOut,
    /// The worker's streams failed or carried something that is not the protocol,
    /// which is what a worker dying looks like.
    Failed(FrameError),
}

/// A running worker. Dropping it kills and reaps the process.
struct Worker {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Worker {
    fn start(command: &WorkerCommand, timeout: Duration) -> Result<Self, IsolationError> {
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
        let introduction = worker.bounded(timeout, |_, output| read_frame::<Response>(output));
        match introduction {
            Err(Exchange::TimedOut) => Err(IsolationError::Timeout {
                after: timeout,
                worker: worker.child.id(),
            }),
            Ok(Some(Response::Ready {
                protocol,
                engine_version,
            })) => accept(protocol, &engine_version).map(|()| worker),
            Ok(_) => Err(IsolationError::Handshake(
                "it did not introduce itself".into(),
            )),
            Err(Exchange::Failed(e)) => Err(IsolationError::Handshake(e.to_string())),
        }
    }

    fn exchange(
        &mut self,
        files: &[SourceFile],
        timeout: Duration,
    ) -> Result<Vec<ExtractOutcome>, Exchange> {
        let response = self.bounded(timeout, |input, output| {
            write_frame(input, &RequestRef::Extract(files))?;
            input.flush()?;
            read_frame::<Response>(output)
        })?;
        match response {
            Some(Response::Extracted(outcomes)) if outcomes.len() == files.len() => {
                Ok(outcomes.into_iter().map(ExtractOutcome::from).collect())
            }
            Some(_) => Err(Exchange::Failed(FrameError::Malformed(
                "a response that does not answer the batch".into(),
            ))),
            None => Err(Exchange::Failed(FrameError::Truncated)),
        }
    }

    /// Runs `talk` over the worker's streams on a thread of its own, and waits for it
    /// at most `timeout`. Past that, the worker is killed and reaped, which closes the
    /// pipes the thread is blocked on, whether writing or reading; the thread then
    /// ends, and is joined, before this returns. Nothing the thread read is returned
    /// then: the worker is not used again.
    fn bounded<T: Send>(
        &mut self,
        timeout: Duration,
        talk: impl FnOnce(
            &mut BufWriter<ChildStdin>,
            &mut BufReader<ChildStdout>,
        ) -> Result<T, FrameError>
        + Send,
    ) -> Result<T, Exchange> {
        let (input, output, child) = (&mut self.input, &mut self.output, &mut self.child);
        std::thread::scope(|scope| {
            let (done, result) = mpsc::sync_channel(1);
            scope.spawn(move || {
                // The receiver is gone only once the wait below is over, timed out.
                let _ = done.send(talk(input, output));
            });
            match result.recv_timeout(timeout) {
                Ok(answer) => answer.map_err(Exchange::Failed),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(Exchange::TimedOut)
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // The thread ended without an answer: it panicked, which the scope
                    // passes on when it joins the thread.
                    Err(Exchange::Failed(FrameError::Truncated))
                }
            }
        })
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

/// Accepts a worker's introduction only if it speaks this protocol for this engine: a
/// worker of another protocol would exchange messages of another shape.
fn accept(protocol: u32, engine_version: &str) -> Result<(), IsolationError> {
    let ours = Engine::version();
    if protocol != PROTOCOL || engine_version != ours {
        return Err(IsolationError::Handshake(format!(
            "it speaks protocol {protocol} for engine {engine_version}, \
             not protocol {PROTOCOL} for engine {ours}"
        )));
    }
    Ok(())
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

/// Writes `value` as one frame. Its length comes from a pass that only counts, and the
/// value is then encoded straight into `w`, so a batch's sources are never copied into
/// a buffer of their own on the way.
fn write_frame<T: Serialize>(w: &mut impl Write, value: &T) -> Result<(), FrameError> {
    let malformed = |e: postcard::Error| FrameError::Malformed(e.to_string());
    let len =
        postcard::serialize_with_flavor::<T, _, _>(value, postcard::ser_flavors::Size::default())
            .map_err(malformed)? as u64;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    w.write_all(&len.to_le_bytes())?;
    postcard::to_io(value, &mut *w).map_err(|_| {
        FrameError::Io(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the stream refused part of a frame",
        ))
    })?;
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
    fn a_worker_of_another_protocol_is_refused() {
        // 6 since a definition carries every route binding with its declaring node
        // (issue 54): a worker still speaking 5 (one route, issues 46 and 47) sends
        // extractions of the old shape.
        assert_eq!(PROTOCOL, 6);
        let ours = Engine::version();
        for (protocol, version) in [
            (2, ours.as_str()),
            (3, ours.as_str()),
            (4, ours.as_str()),
            (5, ours.as_str()),
            (7, ours.as_str()),
            (PROTOCOL, "0"),
        ] {
            let err = accept(protocol, version).unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("protocol {protocol} for engine {version}")),
                "{err}"
            );
        }
        accept(PROTOCOL, &ours).expect("this program's own workers are accepted");
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
