// Compatibility executable for scripts that still invoke the pre-rename
// command name. Keep the implementation in one file so both binaries expose
// exactly the same CLI behavior.
include!("agentdock-cli.rs");
