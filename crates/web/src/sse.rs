//! SSE 实时流：engine 事件转发 + 1s 节流 progress 快照（含 token 速率）。

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use tokio_stream::Stream;

use crate::{ApiError, AppState, project_view};

/// SSE 事件流：`item` / `status` 来自 engine 广播，`progress` 每 1s 合成一次。
/// progress 用 interval 固定节拍：若用 select 内新建 sleep，运行期间 item 事件
/// 会不断重置计时，导致 progress 被饿死（前端在整个运行期间收不到状态更新）。
pub async fn events(
    State(st): State<Arc<AppState>>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    let mut rx = st.events.subscribe();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let stream = async_stream::stream! {
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(ev) => {
                        let name = ev.event_name();
                        let data = serde_json::to_string(&ev).unwrap_or_default();
                        yield Ok(Event::default().event(name).data(data));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    let data = project.view_snapshot();
                    st.update_rate(&data).await;
                    let rate = st.rate_snapshot().await;
                    let view = project_view(&data, rate);
                    let data = serde_json::to_string(&view).unwrap_or_default();
                    yield Ok(Event::default().event("progress").data(data));
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
