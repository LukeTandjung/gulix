//! Helix with Typst previews when running inside Tern.

use anyhow::{Context, Result};
use helix_term::args::Args;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse_args().context("could not parse arguments")?;
    let simple_edit = args.files.len() <= 1
        && args.split.is_none()
        && !args.load_tutor
        && !args.strict
        && args.verbosity == 0
        && args.log_file.is_none()
        && args.config_file.is_none()
        && args.working_directory.is_none()
        && !args.display_help
        && !args.display_version
        && !args.health
        && !args.fetch_grammars
        && !args.build_grammars;

    if simple_edit {
        let path = args.files.first().map(|(path, _)| path.clone());
        if helix_typst::run(path.as_deref())? {
            return Ok(());
        }
    }

    let exit_code = helix_term::entry::run(args).await?;
    std::process::exit(exit_code);
}
