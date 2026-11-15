use crate::{BuildInfo, Context};

/// Return structured build metadata for human or machine rendering.
#[must_use]
pub fn version(ctx: &Context) -> BuildInfo {
    ctx.build().clone()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn returns_context_build_information() {
        let build = BuildInfo::new("1.2.3", Some("abc123"), "aarch64", "macos");
        let ctx = Context::new(build.clone(), PathBuf::from("/tmp"));

        assert_eq!(version(&ctx), build);
    }
}
