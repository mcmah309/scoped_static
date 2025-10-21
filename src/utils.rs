pub(crate) fn abort() -> ! {
    const ROOT_MSG: &str = "Fatal error: Scope dropped while Lifted references still exist. \
                This would cause undefined behavior. Aborting.\n";
    // We don't panic since panics can be recovered and panics also only effect a single thread.
    // While the value could have been sent to a different thread.
    #[cfg(not(feature = "test"))]
    {
        let bt = std::backtrace::Backtrace::capture();
        let msg = match bt.status() {
            std::backtrace::BacktraceStatus::Unsupported => ROOT_MSG.to_owned(),
            std::backtrace::BacktraceStatus::Disabled => {
                format!("{ROOT_MSG}\n(Hint: re-run with `RUST_BACKTRACE=1` to see a backtrace.)\n")
            }
            std::backtrace::BacktraceStatus::Captured => {
                format!("{ROOT_MSG}\nBacktrace:\n{bt}\n")
            }
            _ => ROOT_MSG.to_owned(),
        };
        use std::io::Write;
        let _ = std::io::stderr().write_all(msg.as_bytes());
        let _ = std::io::stderr().flush();
        std::process::abort();
    }
    #[cfg(feature = "test")]
    {
        panic!("{}", ROOT_MSG);
    }
}
