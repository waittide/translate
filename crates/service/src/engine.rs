//! 滑动窗口翻译引擎：并发调度 + 取消。
//!
//! 工程状态由 [`crate::project::ProjectHandle`] 对应的 Actor 单写者持有；
//! 引擎只负责从 Actor 领取任务、并发调用 LLM、把结果交回 Actor。

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use llm::LlmClient;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    ServiceError, TranslateReport,
    project::{ProjectHandle, now_ms},
};

/// 每处理这么多条落盘一次。
const SAVE_EVERY: usize = 10;
/// 距上次落盘超过此时长也落盘。
const SAVE_INTERVAL: Duration = Duration::from_secs(2);

/// 执行一轮滑动窗口翻译，直至 waiting 清空或取消。
pub(crate) async fn run(
    client: Arc<LlmClient>,
    project: ProjectHandle,
    cancel: CancellationToken,
) -> Result<TranslateReport, ServiceError> {
    let Some(meta) = project.prepare_run().await? else {
        return Ok(TranslateReport {
            attempts:  0,
            succeeded: 0,
            failed:    0,
            cancelled: false,
        });
    };

    let mut waiting_total = meta.waiting_total;
    let concurrency = meta.concurrency;
    let timeout = meta.timeout;
    let accumulated_ms = meta.accumulated_ms;

    let started = Instant::now();
    let mut tasks = JoinSet::new();
    let mut in_flight = 0usize;
    let mut completed = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut last_save = Instant::now();

    loop {
        // 补任务：从 Actor 领取一批可试条目，锁外 spawn。
        let mut backoff_until: Option<u64> = None;
        if in_flight < concurrency && waiting_total > 0 && !cancel.is_cancelled() {
            let max = concurrency - in_flight;
            let batch = project.take_retryable(now_ms(), max).await?;
            backoff_until = batch.backoff_until;
            for item in batch.items {
                waiting_total -= 1;
                in_flight += 1;
                let segments = BTreeMap::from([(item.line, item.source.clone())]);
                let client = client.clone();
                let file = item.file;
                let line = item.line;
                let retry_count = item.retry_times;
                let source_lang = item.source_language;
                let target_lang = item.target_language;
                // 限流等待在 translate 内部但不计入 timeout，避免配额打满时误报超时
                tasks.spawn(async move {
                    let out = client
                        .translate(source_lang, target_lang, segments, Some(timeout))
                        .await;
                    (file, line, retry_count, out)
                });
            }
        }

        if in_flight == 0 {
            if let Some(until) = backoff_until {
                // 全部在退避期：等到最早可重试或取消
                let dur = Duration::from_millis(until.saturating_sub(now_ms()));
                tokio::select! {
                    _ = tokio::time::sleep(dur) => continue,
                    _ = cancel.cancelled() => break,
                }
            }
            break;
        }

        let Some(joined) = tasks.join_next().await else { break };
        match joined {
            Ok((file, line, retry_count, out)) => {
                in_flight -= 1;
                completed += 1;
                match out {
                    Ok(resp) => {
                        if let Some(target) = resp.segments.get(&line) {
                            project
                                .complete_success(&file, line, target.clone(), resp.usage)
                                .await?;
                            succeeded += 1;
                        } else {
                            let reason = "模型未返回该编号".to_string();
                            let outcome = project
                                .complete_failure(&file, line, retry_count, reason, now_ms())
                                .await?;
                            if outcome.requeued {
                                waiting_total += 1;
                            } else {
                                failed += 1;
                            }
                        }
                    }
                    Err(err) => {
                        let reason = err.to_string();
                        let outcome = project
                            .complete_failure(&file, line, retry_count, reason, now_ms())
                            .await?;
                        if outcome.requeued {
                            waiting_total += 1;
                        } else {
                            failed += 1;
                        }
                    }
                }
                // 双条件节流落盘：条数或时间
                if completed.is_multiple_of(SAVE_EVERY) || last_save.elapsed() >= SAVE_INTERVAL {
                    project
                        .save(accumulated_ms + started.elapsed().as_millis() as u64)
                        .await?;
                    last_save = Instant::now();
                }
            }
            Err(err) => {
                // JoinSet 任务 panic：先落盘 Paused 再冒泡，避免工程卡在 Running 无法恢复
                project.complete_panic().await?;
                return Err(ServiceError::TaskPanic(err.to_string()));
            }
        }
    }

    let cancelled = cancel.is_cancelled();
    project
        .finish_run(cancelled, accumulated_ms + started.elapsed().as_millis() as u64)
        .await?;

    Ok(TranslateReport {
        attempts: completed,
        succeeded,
        failed,
        cancelled,
    })
}
