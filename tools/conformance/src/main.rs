//! The cadrs adapter: answers the B-rep protocol's requests on stdin and stdout, so the
//! conformance suite, or any other client, can drive cadrs's kernel.

use std::process::ExitCode;

fn main() -> ExitCode {
    match brep_protocol::serve(&cadrs_conformance::Cadrs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
