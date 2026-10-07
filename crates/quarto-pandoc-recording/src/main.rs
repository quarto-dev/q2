use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use quarto_pandoc_recording::{replay, rewrite};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Normalize and replay recordings of native pandoc runs")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Rewrite a raw capture run into `<set-root>/<name>`.
    Rewrite {
        raw_run: PathBuf,
        set_root: PathBuf,
        name: String,
        /// Also replay at the canonical work dir and store that output as the reference.
        #[arg(long)]
        pandoc: Option<PathBuf>,
    },
    /// Replay `<set-root>/<name>` with a native pandoc; exit 1 if the output
    /// is not byte-equal to the recorded reference.
    Replay {
        set_root: PathBuf,
        name: String,
        #[arg(long, default_value = "pandoc")]
        pandoc: PathBuf,
        /// Write the replayed output here.
        #[arg(long)]
        out: Option<PathBuf>,
        /// `NAME=VALUE` sets (a bare `NAME` removes) an env var for this replay.
        #[arg(long = "env")]
        env: Vec<String>,
        /// Extra pandoc argument inserted before the recorded ones.
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
        /// Skip the byte comparison against the reference (exploration).
        #[arg(long)]
        no_compare: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Rewrite {
            raw_run,
            set_root,
            name,
            pandoc,
        } => rewrite::rewrite_run(&raw_run, &set_root, &name, pandoc.as_deref()),
        Cmd::Replay {
            set_root,
            name,
            pandoc,
            out,
            env,
            args,
            no_compare,
        } => {
            let work = replay::canonical_work_dir(&name);
            let overrides = replay::Overrides { env, args };
            let res =
                replay::replay_with(&set_root.join(&name), &set_root, &pandoc, &work, &overrides)?;
            if let (Some(p), Some(bytes)) = (&out, &res.output) {
                std::fs::write(p, bytes)?;
            }
            if res.status != 0 {
                bail!("pandoc exited {}: {}", res.status, res.stderr);
            }
            if !no_compare && res.output != res.reference {
                bail!("replayed output differs from the reference");
            }
            println!("byte-equal");
            Ok(())
        }
    }
}
