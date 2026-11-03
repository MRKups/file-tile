//! Native estimates never share listing, metadata or persistence worker admission.
use super::work::{WorkError, Workers};
use crate::application::{
    contracts::directory::Cancellation,
    files::folder_usage::{Estimate, UsageCompletion, UsageError, UsageReport, UsageRequest},
};
use crate::diagnostics::{Activity, Outcome, SizeErrors, SizeIssue, Subsystem, session};
use crate::platform::folder_usage::{UsageFailure, UsageIssue};
use crate::platform::permissions::PermissionStatus;

pub struct UsageWorkers {
    workers: Workers<Result<UsageReport, UsageError>>,
}

impl UsageWorkers {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("folder-usage", 2, 2)?,
        })
    }

    pub fn submit(
        &self,
        request: UsageRequest,
    ) -> (Cancellation, impl Future<Output = UsageCompletion> + use<>) {
        self.submit_with_exclusions(request, Vec::new(), false)
    }

    pub fn submit_with_exclusions(
        &self,
        request: UsageRequest,
        mut excluded: Vec<&'static str>,
        probe_home_access: bool,
    ) -> (Cancellation, impl Future<Output = UsageCompletion> + use<>) {
        let directory = request.directory.clone();
        let generation = request.generation;
        let future = self.workers.submit(move |cancel| {
            if probe_home_access {
                for target in [
                    crate::domain::ProtectedAccess::Desktop,
                    crate::domain::ProtectedAccess::Documents,
                    crate::domain::ProtectedAccess::Downloads,
                ] {
                    if let Some(name) = target.home_folder_name()
                        && !excluded.contains(&name)
                        && crate::platform::permissions::check_folder_access(Some(name), &directory)
                            != PermissionStatus::Granted
                    {
                        excluded.push(name);
                    }
                }
            }
            let (raw, issue, elapsed) = crate::platform::folder_usage::measure_with_exclusions(
                &directory,
                &excluded,
                || cancel.is_cancelled(),
            );
            let result = raw
                .map(|report| UsageReport {
                    total: Estimate {
                        bytes: report.total_bytes,
                        partial: report.total_partial,
                    },
                    children: report
                        .children
                        .into_iter()
                        .map(|(path, bytes, partial)| (path, Estimate { bytes, partial }))
                        .collect(),
                })
                .map_err(|failure| match failure {
                    UsageFailure::Cancelled => UsageError::Cancelled,
                    UsageFailure::TimedOut => UsageError::TimedOut,
                    UsageFailure::Unavailable => UsageError::Unavailable,
                    UsageFailure::InvalidOutput => UsageError::InvalidOutput,
                });
            if !matches!(result, Err(UsageError::Cancelled))
                && let Some(cause) = issue.map(map_issue)
            {
                session().record(
                    Subsystem::Size,
                    Activity::SizeIssue(cause),
                    if result.is_ok() {
                        Outcome::Partial
                    } else {
                        Outcome::Unavailable
                    },
                    generation,
                    0,
                    Some(elapsed),
                );
            }
            result
        });
        let cancellation = future.cancellation();
        (cancellation, async move {
            UsageCompletion {
                request,
                result: future.await.unwrap_or_else(|error| {
                    Err(match error {
                        WorkError::Cancelled => UsageError::Cancelled,
                        WorkError::Busy | WorkError::Unavailable => UsageError::Unavailable,
                    })
                }),
            }
        })
    }
}

fn map_issue(issue: UsageIssue) -> SizeIssue {
    match issue {
        UsageIssue::TimedOut => SizeIssue::TimedOut,
        UsageIssue::Unavailable => SizeIssue::Unavailable,
        UsageIssue::OutputLimit => SizeIssue::OutputLimit,
        UsageIssue::InvalidOutput => SizeIssue::InvalidOutput,
        UsageIssue::UtilityExit { code, errors } => SizeIssue::UtilityExit {
            code,
            errors: SizeErrors {
                access_denied: u64::from(errors.access_denied),
                not_found: u64::from(errors.not_found),
                io: u64::from(errors.io),
                other: u64::from(errors.other),
            },
        },
    }
}
