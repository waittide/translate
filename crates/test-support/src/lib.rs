//! 测试用假 HTTP 服务：监听本地随机端口，按 handler 返回 JSON 响应。
//! 供 llm / service / web 集成测试共用。
//!
//! handler 为异步：每连接在独立 task 中处理，互不阻塞；用 `tokio::time::sleep` 模拟延迟，
//! 避免阻塞 runtime 线程导致测试定时器/引擎被饿死。

use std::{collections::HashMap, future::Future};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

/// 收到的请求：请求行路径 + 小写 header + body。
#[derive(Clone)]
pub struct Request {
    pub path:    String,
    pub headers: HashMap<String, String>,
    pub body:    String,
}

/// 固定响应。
pub async fn spawn(response_body: &'static str, status: u16) -> (String, mpsc::Receiver<Request>) {
    spawn_handler(move |_| async move { (status, response_body.to_string()) }).await
}

/// 自定义异步响应：handler 返回 (status, body)；每连接独立 task 并发处理。
pub async fn spawn_handler<F, Fut>(handler: F) -> (String, mpsc::Receiver<Request>)
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = (u16, String)> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel(64);
    let handler = std::sync::Arc::new(handler);
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            let tx = tx.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                let mut sock = sock;
                let Some(req) = read_request(&mut sock).await else {
                    return;
                };
                let record = req.clone();
                let (status, body) = handler(req).await;
                let _ = tx.send(record).await;
                let resp = format!(
                    "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (format!("http://{addr}"), rx)
}

/// 读取一个请求：请求行路径 + 小写 header + body（按 content-length 收全）。
async fn read_request(sock: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let len = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if buf.len() >= end + 4 + len {
                break;
            }
        }
    }
    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let path = lines.next()?.split_whitespace().nth(1)?.to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    Some(Request { path, headers, body })
}
