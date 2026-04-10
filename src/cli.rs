use tokio::io::AsyncBufReadExt;
use tokio::sync::broadcast;

use crate::error::MossError;
use crate::moss::signal::{self, Event};
use crate::Moss;

pub struct Cli {
    moss: Moss,
    rx: broadcast::Receiver<signal::Payload>,
}

impl Cli {
    pub fn new(moss: Moss) -> Self {
        let rx = moss.subscribe();
        Self { moss, rx }
    }

    pub async fn run(&mut self) -> Result<(), MossError> {
        let stdin = tokio::io::stdin();
        let mut lines = tokio::io::BufReader::new(stdin).lines();

        loop {
            match lines.next_line().await? {
                Some(raw) => self.handle_input(raw.trim_end()).await?,
                None => break,
            }
        }

        Ok(())
    }

    async fn handle_input(&mut self, input: &str) -> Result<(), MossError> {
        match input {
            "" => {}
            "exit" | "quit" => std::process::exit(0),
            query => {
                tokio::pin!(let fut = self.moss.run(query););
                loop {
                    tokio::select! {
                        result = &mut fut => {
                            match result {
                                Ok(response) => println!("{response}"),
                                Err(e) => eprintln!("[moss] error: {e}"),
                            }
                            break;
                        }
                        event = self.rx.recv() => match event {
                            Ok(Event::Snapshot(json)) => {
                                tracing::debug!(snapshot = %json, "board updated");
                            }
                            Ok(Event::ApprovalRequested { gap_id, gap_name, reason }) => {
                                eprintln!("\n[guard] gap `{gap_name}` requires approval");
                                eprintln!("       reason: {reason}");
                                eprint!("       approve? [y/N] ");

                                let mut line = String::new();
                                tokio::io::AsyncBufReadExt::read_line(
                                    &mut tokio::io::BufReader::new(tokio::io::stdin()),
                                    &mut line,
                                ).await?;

                                let approved = line.trim().eq_ignore_ascii_case("y");
                                self.moss.approve(gap_id, approved);
                            }
                            Ok(Event::QuestionAsked { gap_id, gap_name, question }) => {
                                eprintln!("\n[guard] gap `{gap_name}` requires an answer");
                                eprintln!("       question: {question}");
                                eprint!("       answer: ");

                                let mut line = String::new();
                                tokio::io::AsyncBufReadExt::read_line(
                                    &mut tokio::io::BufReader::new(tokio::io::stdin()),
                                    &mut line,
                                ).await?;

                                self.moss.answer(gap_id, line.trim().to_string());
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(skipped = n, "signal bus lagged");
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        },
                    }
                }
            }
        }
        Ok(())
    }
}
