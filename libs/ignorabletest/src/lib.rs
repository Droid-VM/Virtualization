//! Test harness which supports ignoring tests at runtime.

#[cfg(test)]
mod runner;

/// Macro to generate the main function for the test harness.
#[macro_export]
macro_rules! test_main {
    ($tests:expr) => {
        #[cfg(test)]
        fn main() {
            ignorabletest::runner::main($tests)
        }
    };
}
