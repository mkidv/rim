#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;

use core::fmt;

// SPDX-License-Identifier: MIT
// core/verify/types.rs
use bitflags::bitflags;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

impl PartialOrd for Severity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Severity {
    fn cmp(&self, other: &Self) -> Ordering {
        use Severity::*;
        fn rank(s: Severity) -> u8 {
            match s {
                Info => 0,
                Warn => 1,
                Error => 2,
            }
        }
        rank(*self).cmp(&rank(*other))
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub sev: Severity,
    pub code: &'static str,
    pub msg: String,
}
impl Finding {
    pub fn info(code: &'static str, msg: impl Into<String>) -> Self {
        Self {
            sev: Severity::Info,
            code,
            msg: msg.into(),
        }
    }
    pub fn warn(code: &'static str, msg: impl Into<String>) -> Self {
        Self {
            sev: Severity::Warn,
            code,
            msg: msg.into(),
        }
    }
    pub fn err(code: &'static str, msg: impl Into<String>) -> Self {
        Self {
            sev: Severity::Error,
            code,
            msg: msg.into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VerifyReport {
    pub findings: Vec<Finding>,
}

impl VerifyReport {
    pub fn has_error(&self) -> bool {
        self.findings
            .iter()
            .any(|f| matches!(f.sev, Severity::Error))
    }

    pub fn first_error(&self) -> Option<&str> {
        self.findings
            .iter()
            .find(|f| matches!(f.sev, Severity::Error))
            .map(|f| f.msg.as_str())
    }

    pub fn ok(&self) -> bool {
        !self.has_error()
    }

    pub fn push(&mut self, f: Finding) {
        self.findings.push(f)
    }
    pub fn count(&self, s: Severity) -> usize {
        self.findings.iter().filter(|f| f.sev == s).count()
    }

    /// Display with options (filtering, prefix, summary...)
    pub fn display_with<'a>(&'a self, opts: ReportDisplayOpts) -> ReportDisplay<'a> {
        ReportDisplay::new(self, opts)
    }

    /// Display "only errors", default prefix, no summary
    pub fn errors_only<'a>(&'a self) -> ReportDisplay<'a> {
        self.display_with(ReportDisplayOpts {
            min_level: Severity::Error,
            ..ReportDisplayOpts::default()
        })
    }

    /// Display "warn + error"
    pub fn warn_and_errors<'a>(&'a self) -> ReportDisplay<'a> {
        self.display_with(ReportDisplayOpts {
            min_level: Severity::Warn,
            ..ReportDisplayOpts::default()
        })
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ReportDisplayOpts {
    pub min_level: Severity,
    pub prefix: &'static str,
    pub show_summary: bool,
    pub pad_code: usize,
}

impl ReportDisplayOpts {
    fn new(min_level: Severity, prefix: &'static str, show_summary: bool, pad_code: usize) -> Self {
        Self {
            min_level,
            prefix,
            show_summary,
            pad_code,
        }
    }
}

impl Default for ReportDisplayOpts {
    fn default() -> Self {
        Self::new(Severity::Info, "", false, 12)
    }
}

pub struct ReportDisplay<'a> {
    rep: &'a VerifyReport,
    opts: ReportDisplayOpts,
}

impl<'a> ReportDisplay<'a> {
    pub fn new(rep: &'a VerifyReport, opts: ReportDisplayOpts) -> Self {
        Self { rep, opts }
    }
}

impl<'a> core::fmt::Display for ReportDisplay<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut n_info = 0usize;
        let mut n_warn = 0usize;
        let mut n_err = 0usize;

        for it in &self.rep.findings {
            if it.sev < self.opts.min_level {
                continue;
            }
            let tag = match it.sev {
                Severity::Info => "INFO",
                Severity::Warn => "WARN",
                Severity::Error => "ERR ",
            };
            match it.sev {
                Severity::Info => n_info += 1,
                Severity::Warn => n_warn += 1,
                Severity::Error => n_err += 1,
            }

            writeln!(
                f,
                "{}{tag}: {:<width$} {}",
                self.opts.prefix,
                it.code,
                it.msg,
                width = self.opts.pad_code
            )?;
        }

        if self.opts.show_summary {
            writeln!(
                f,
                "{}Summary: errors={}  warns={}  infos={}",
                self.opts.prefix, n_err, n_warn, n_info
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for VerifyReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        ReportDisplay {
            rep: self,
            opts: ReportDisplayOpts::default(),
        }
        .fmt(f)
    }
}

bitflags! {
    #[derive(Clone, Debug)]
    pub struct VerifyPhases: u32 {
        const BOOT       = 1 << 0;
        const GEOMETRY   = 1 << 1;
        const CHAIN        = 1 << 2;
        const ROOT       = 1 << 3;
        const CROSSREF   = 1 << 4;
        const CONTENT    = 1 << 5;
        const CUSTOM     = 1 << 6; // free for FS
        const ALL        = u32::MAX;
    }
}

/// Generic options that the FS can encapsulate/extend.
pub trait VerifierOptionsLike {
    fn phases(&self) -> VerifyPhases {
        VerifyPhases::ALL
    }
    fn fail_fast(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
pub struct CoreVerifyOptions {
    pub phases: VerifyPhases,
    pub fail_fast: bool,
}

impl VerifierOptionsLike for CoreVerifyOptions {
    fn phases(&self) -> VerifyPhases {
        self.phases.clone()
    }
    fn fail_fast(&self) -> bool {
        self.fail_fast
    }
}

impl Default for CoreVerifyOptions {
    fn default() -> Self {
        Self {
            phases: VerifyPhases::ALL,
            fail_fast: false,
        }
    }
}
