use std::collections::BTreeMap;
use std::ffi::OsString;
use std::process::Stdio;

use tokio::process::Command;
use tokio::sync::oneshot;

use crate::{Error, Result, Server};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandResult {
    pub success: bool,
    pub exit_code: Option<i32>,
}

impl Server {
    /// Run one caller process while this server owns the listener.
    ///
    /// The listener is already bound when the process starts, removing the readiness race common
    /// to background-server test scripts. Standard streams are inherited, the process status is
    /// preserved, and the server receives graceful shutdown after the process exits.
    pub async fn run_command(
        self,
        command: Vec<OsString>,
        environment: BTreeMap<String, String>,
    ) -> Result<CommandResult> {
        let (program, arguments) = command.split_first().ok_or(Error::CommandMissing)?;
        let display_program = program.to_string_lossy().into_owned();
        let mut child = Command::new(program);
        child
            .args(arguments)
            .envs(environment)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = child.spawn().map_err(|source| Error::CommandSpawn {
            command: display_program,
            source,
        })?;

        let (shutdown, shutdown_requested) = oneshot::channel::<()>();
        let mut server_task = tokio::spawn(self.run_until(async move {
            drop(shutdown_requested.await);
        }));

        enum Completion {
            Child(std::io::Result<std::process::ExitStatus>),
            Interrupted(std::io::Result<()>),
            Server(std::result::Result<Result<()>, tokio::task::JoinError>),
        }

        let completion = tokio::select! {
            status = child.wait() => Completion::Child(status),
            signal = tokio::signal::ctrl_c() => Completion::Interrupted(signal),
            result = &mut server_task => Completion::Server(result),
        };

        match completion {
            Completion::Child(status) => {
                let _ = shutdown.send(());
                server_task.await.map_err(Error::CommandServiceTask)??;
                let status = status.map_err(Error::CommandWait)?;
                Ok(CommandResult {
                    success: status.success(),
                    exit_code: status.code(),
                })
            }
            Completion::Interrupted(signal) => {
                signal.map_err(Error::CommandSignal)?;
                drop(child.start_kill());
                drop(child.wait().await);
                let _ = shutdown.send(());
                server_task.await.map_err(Error::CommandServiceTask)??;
                Ok(CommandResult {
                    success: false,
                    exit_code: None,
                })
            }
            Completion::Server(result) => {
                drop(child.start_kill());
                drop(child.wait().await);
                match result.map_err(Error::CommandServiceTask)? {
                    Ok(()) => Err(Error::CommandServiceStopped),
                    Err(error) => Err(error),
                }
            }
        }
    }
}
