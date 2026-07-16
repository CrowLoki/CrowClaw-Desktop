use std::{
    cmp::min,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::Mutex,
};

use crate::agent::CancellationToken;

use super::{
    approval::{ApprovalRegistry, ClaimedAction},
    ApprovalDecision, ApprovalStatus, ApprovalToken, DirectoryEntry, DirectoryEntryKind,
    ProposedAction, ToolError, ToolExecution, ToolOutput, ToolRequest,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicy {
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
    pub max_file_bytes: usize,
    pub max_output_bytes: usize,
    pub max_directory_entries: usize,
    pub command_timeout_ms: u64,
    pub allow_commands: bool,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            allowed_roots: Vec::new(),
            max_file_bytes: 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            max_directory_entries: 2_000,
            command_timeout_ms: 30_000,
            allow_commands: false,
        }
    }
}

impl ToolPolicy {
    pub fn for_roots(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            allowed_roots: roots.into_iter().collect(),
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<(), ToolError> {
        if self.max_file_bytes == 0 {
            return Err(ToolError::InvalidRequest {
                tool_name: "policy".into(),
                message: "max_file_bytes must be greater than zero".into(),
            });
        }
        if self.max_output_bytes == 0 {
            return Err(ToolError::InvalidRequest {
                tool_name: "policy".into(),
                message: "max_output_bytes must be greater than zero".into(),
            });
        }
        if self.max_directory_entries == 0 {
            return Err(ToolError::InvalidRequest {
                tool_name: "policy".into(),
                message: "max_directory_entries must be greater than zero".into(),
            });
        }
        if self.command_timeout_ms == 0 {
            return Err(ToolError::InvalidRequest {
                tool_name: "policy".into(),
                message: "command_timeout_ms must be greater than zero".into(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ToolExecutor {
    policy: ToolPolicy,
    approvals: Arc<ApprovalRegistry>,
}

impl ToolExecutor {
    pub fn new(policy: ToolPolicy) -> Result<Self, ToolError> {
        policy.validate()?;
        Ok(Self {
            policy,
            approvals: Arc::new(ApprovalRegistry::default()),
        })
    }

    pub fn policy(&self) -> &ToolPolicy {
        &self.policy
    }

    /// Records a proposal only. This method performs no filesystem or process I/O.
    pub fn propose(&self, request: ToolRequest) -> Result<ProposedAction, ToolError> {
        self.approvals.propose(request)
    }

    /// Resolves the user's choice only. Approval does not itself execute the action.
    pub fn resolve(
        &self,
        token: &ApprovalToken,
        decision: ApprovalDecision,
    ) -> Result<ApprovalStatus, ToolError> {
        self.approvals.resolve(token, decision)
    }

    pub fn status(&self, token: &ApprovalToken) -> Result<ApprovalStatus, ToolError> {
        self.approvals.status(token)
    }

    /// Atomically consumes a resolved token, then and only then performs the action.
    pub async fn execute(
        &self,
        token: &ApprovalToken,
        cancellation: &CancellationToken,
    ) -> Result<ToolExecution, ToolError> {
        match self.approvals.claim(token)? {
            ClaimedAction::Denied(proposal, reason) => Ok(ToolExecution::Denied {
                action_id: proposal.action_id,
                reason,
            }),
            ClaimedAction::Execute(proposal) => {
                let action_id = proposal.action_id.clone();
                if cancellation.is_cancelled() {
                    return Ok(ToolExecution::Failed {
                        action_id,
                        error: ToolError::Cancelled,
                    });
                }

                match self.perform(proposal.request, cancellation).await {
                    Ok(output) => Ok(ToolExecution::Executed { action_id, output }),
                    Err(error) => Ok(ToolExecution::Failed { action_id, error }),
                }
            }
        }
    }

    async fn perform(
        &self,
        request: ToolRequest,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        match request {
            ToolRequest::ListDirectory { path } => self.list_directory(&path, cancellation).await,
            ToolRequest::ReadTextFile { path } => self.read_text_file(&path, cancellation).await,
            ToolRequest::RunCommand { program, args, cwd } => {
                self.run_command(program, args, cwd, cancellation).await
            }
        }
    }

    async fn list_directory(
        &self,
        requested_path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let path = self.authorize_path(requested_path).await?;
        let mut directory = tokio::fs::read_dir(&path)
            .await
            .map_err(|error| ToolError::file_system("list directory", &path, error))?;
        let mut entries = Vec::new();
        let mut encoded_bytes = 0usize;
        let mut truncated = false;

        loop {
            let next = tokio::select! {
                _ = cancellation.cancelled() => return Err(ToolError::Cancelled),
                result = directory.next_entry() => result,
            }
            .map_err(|error| ToolError::file_system("read directory entry", &path, error))?;

            let Some(entry) = next else {
                break;
            };
            if entries.len() >= self.policy.max_directory_entries {
                truncated = true;
                break;
            }

            let file_type = entry.file_type().await.map_err(|error| {
                ToolError::file_system("inspect directory entry", &entry.path(), error)
            })?;
            let kind = if file_type.is_symlink() {
                DirectoryEntryKind::Symlink
            } else if file_type.is_dir() {
                DirectoryEntryKind::Directory
            } else if file_type.is_file() {
                DirectoryEntryKind::File
            } else {
                DirectoryEntryKind::Other
            };
            let item = DirectoryEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                kind,
            };
            let item_bytes = serde_json::to_vec(&item)
                .map_err(|error| ToolError::CommandIo {
                    message: error.to_string(),
                })?
                .len();
            if encoded_bytes.saturating_add(item_bytes) > self.policy.max_output_bytes {
                truncated = true;
                break;
            }
            encoded_bytes += item_bytes;
            entries.push(item);
        }

        entries.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
        });
        Ok(ToolOutput::DirectoryListing {
            path,
            entries,
            truncated,
        })
    }

    async fn read_text_file(
        &self,
        requested_path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let path = self.authorize_path(requested_path).await?;
        let limit = min(self.policy.max_file_bytes, self.policy.max_output_bytes);
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|error| ToolError::file_system("open text file", &path, error))?;
        let mut bytes = Vec::with_capacity(min(limit, 64 * 1024));
        let mut limited_file = file.take(limit as u64 + 1);
        let read = limited_file.read_to_end(&mut bytes);
        tokio::select! {
            _ = cancellation.cancelled() => return Err(ToolError::Cancelled),
            result = read => result.map_err(|error| ToolError::file_system("read text file", &path, error))?,
        };

        if bytes.len() > limit {
            return Err(ToolError::FileTooLarge {
                path: path.display().to_string(),
                limit_bytes: limit,
            });
        }
        let content = String::from_utf8(bytes).map_err(|_| ToolError::NotText {
            path: path.display().to_string(),
        })?;
        let bytes = content.len();
        Ok(ToolOutput::TextFile {
            path,
            content,
            bytes,
        })
    }

    async fn run_command(
        &self,
        program: String,
        args: Vec<String>,
        requested_cwd: PathBuf,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        if !self.policy.allow_commands {
            return Err(ToolError::CommandsDisabled);
        }
        if program.trim().is_empty() {
            return Err(ToolError::InvalidRequest {
                tool_name: "run_command".into(),
                message: "program cannot be empty".into(),
            });
        }
        let cwd = child_process_path(self.authorize_path(&requested_cwd).await?);

        let mut command = Command::new(&program);
        command
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| ToolError::CommandSpawn {
            program: program.clone(),
            message: error.to_string(),
        })?;

        let stdout = child.stdout.take().ok_or_else(|| ToolError::CommandIo {
            message: "stdout pipe was not created".into(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| ToolError::CommandIo {
            message: "stderr pipe was not created".into(),
        })?;
        let budget = Arc::new(Mutex::new(self.policy.max_output_bytes));
        let stdout_capture = tokio::spawn(capture_limited(stdout, budget.clone()));
        let stderr_capture = tokio::spawn(capture_limited(stderr, budget));

        enum ProcessEnd {
            Exited(std::process::ExitStatus),
            TimedOut,
            Cancelled,
        }

        let process_end = tokio::select! {
            _ = cancellation.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                ProcessEnd::Cancelled
            }
            _ = tokio::time::sleep(Duration::from_millis(self.policy.command_timeout_ms)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                ProcessEnd::TimedOut
            }
            status = child.wait() => {
                ProcessEnd::Exited(status.map_err(|error| ToolError::CommandIo {
                    message: error.to_string(),
                })?)
            }
        };

        let stdout = join_capture(stdout_capture).await?;
        let stderr = join_capture(stderr_capture).await?;
        if matches!(process_end, ProcessEnd::Cancelled) {
            return Err(ToolError::Cancelled);
        }

        let exit_code = match process_end {
            ProcessEnd::Exited(status) => status.code(),
            ProcessEnd::TimedOut | ProcessEnd::Cancelled => None,
        };
        Ok(ToolOutput::Command {
            program,
            exit_code,
            stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
            truncated: stdout.truncated || stderr.truncated,
            timed_out: matches!(process_end, ProcessEnd::TimedOut),
        })
    }

    async fn authorize_path(&self, requested_path: &Path) -> Result<PathBuf, ToolError> {
        if self.policy.allowed_roots.is_empty() {
            return Err(ToolError::NoAllowedRoots);
        }
        let path = tokio::fs::canonicalize(requested_path)
            .await
            .map_err(|error| ToolError::file_system("resolve path", requested_path, error))?;

        for configured_root in &self.policy.allowed_roots {
            let Ok(root) = tokio::fs::canonicalize(configured_root).await else {
                continue;
            };
            if path.starts_with(root) {
                return Ok(path);
            }
        }

        Err(ToolError::PathOutsideAllowedRoots {
            path: path.display().to_string(),
        })
    }
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn capture_limited<R>(
    mut reader: R,
    remaining: Arc<Mutex<usize>>,
) -> Result<CapturedOutput, ToolError>
where
    R: AsyncRead + Unpin,
{
    let mut stored = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    let mut truncated = false;

    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|error| ToolError::CommandIo {
                message: error.to_string(),
            })?;
        if count == 0 {
            break;
        }

        let mut available = remaining.lock().await;
        let keep = min(*available, count);
        stored.extend_from_slice(&buffer[..keep]);
        *available -= keep;
        truncated |= keep < count;
    }

    Ok(CapturedOutput {
        bytes: stored,
        truncated,
    })
}

async fn join_capture(
    handle: tokio::task::JoinHandle<Result<CapturedOutput, ToolError>>,
) -> Result<CapturedOutput, ToolError> {
    handle.await.map_err(|error| ToolError::CommandIo {
        message: format!("output capture task failed: {error}"),
    })?
}

#[cfg(windows)]
fn child_process_path(path: PathBuf) -> PathBuf {
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };

    let wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    const VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const VERBATIM_UNC_PREFIX: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];

    if let Some(rest) = wide.strip_prefix(VERBATIM_UNC_PREFIX) {
        let mut normal = vec![b'\\' as u16, b'\\' as u16];
        normal.extend_from_slice(rest);
        PathBuf::from(OsString::from_wide(&normal))
    } else if let Some(rest) = wide.strip_prefix(VERBATIM_PREFIX) {
        PathBuf::from(OsString::from_wide(rest))
    } else {
        path
    }
}

#[cfg(not(windows))]
fn child_process_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{ToolExecutor, ToolPolicy};
    use crate::{
        agent::CancellationToken,
        tools::{ApprovalDecision, ToolExecution, ToolOutput, ToolRequest},
    };

    #[tokio::test]
    async fn file_content_is_not_read_until_a_resolved_approval_is_consumed() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("evidence.txt");
        fs::write(&file, "before approval").unwrap();
        let executor = ToolExecutor::new(ToolPolicy::for_roots([directory.path().into()])).unwrap();
        let proposal = executor
            .propose(ToolRequest::ReadTextFile { path: file.clone() })
            .unwrap();

        fs::write(&file, "after approval").unwrap();
        assert!(executor
            .execute(&proposal.approval_token, &CancellationToken::new())
            .await
            .is_err());

        executor
            .resolve(&proposal.approval_token, ApprovalDecision::Approve)
            .unwrap();
        let execution = executor
            .execute(&proposal.approval_token, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(
            execution,
            ToolExecution::Executed {
                output: ToolOutput::TextFile { content, .. },
                ..
            } if content == "after approval"
        ));
    }

    #[tokio::test]
    async fn denied_action_never_reads_the_file() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("private.txt");
        fs::write(&file, "private").unwrap();
        let executor = ToolExecutor::new(ToolPolicy::for_roots([directory.path().into()])).unwrap();
        let proposal = executor
            .propose(ToolRequest::ReadTextFile { path: file })
            .unwrap();
        executor
            .resolve(
                &proposal.approval_token,
                ApprovalDecision::Deny {
                    reason: Some("not this file".into()),
                },
            )
            .unwrap();

        let execution = executor
            .execute(&proposal.approval_token, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(
            execution,
            ToolExecution::Denied { reason, .. } if reason == "not this file"
        ));
    }

    #[tokio::test]
    async fn enforces_file_size_boundary_after_approval() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("large.txt");
        fs::write(&file, "12345").unwrap();
        let mut policy = ToolPolicy::for_roots([directory.path().into()]);
        policy.max_file_bytes = 4;
        let executor = ToolExecutor::new(policy).unwrap();
        let proposal = executor
            .propose(ToolRequest::ReadTextFile { path: file })
            .unwrap();
        executor
            .resolve(&proposal.approval_token, ApprovalDecision::Approve)
            .unwrap();

        let execution = executor
            .execute(&proposal.approval_token, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(execution, ToolExecution::Failed { .. }));
    }

    #[tokio::test]
    async fn command_does_not_start_before_approval() {
        let directory = tempdir().unwrap();
        let marker = directory.path().join("command-ran.txt");
        let mut policy = ToolPolicy::for_roots([directory.path().into()]);
        policy.allow_commands = true;
        let executor = ToolExecutor::new(policy).unwrap();
        let request = marker_command(directory.path(), &marker);
        let proposal = executor.propose(request).unwrap();

        assert!(executor
            .execute(&proposal.approval_token, &CancellationToken::new())
            .await
            .is_err());
        assert!(!marker.exists());

        executor
            .resolve(&proposal.approval_token, ApprovalDecision::Approve)
            .unwrap();
        let execution = executor
            .execute(&proposal.approval_token, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(execution, ToolExecution::Executed { .. }));
        assert!(
            marker.exists(),
            "command output: {execution:?}; cwd: {}; canonical cwd: {:?}; directory: {:?}",
            directory.path().display(),
            fs::canonicalize(directory.path()),
            fs::read_dir(directory.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>()
        );
    }

    #[cfg(windows)]
    fn marker_command(cwd: &Path, marker: &Path) -> ToolRequest {
        ToolRequest::RunCommand {
            program: "cmd.exe".into(),
            args: vec![
                "/D".into(),
                "/C".into(),
                format!("echo ran>{}", marker.file_name().unwrap().to_string_lossy()),
            ],
            cwd: cwd.into(),
        }
    }

    #[cfg(not(windows))]
    fn marker_command(cwd: &Path, marker: &Path) -> ToolRequest {
        ToolRequest::RunCommand {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "printf ran > \"$1\"".into(),
                "crowclaw-test".into(),
                marker.display().to_string(),
            ],
            cwd: cwd.into(),
        }
    }
}
