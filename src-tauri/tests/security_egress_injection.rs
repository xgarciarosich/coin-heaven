//! scratch exploration
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
impl Server {
    fn start(respond: impl Fn(&mut TcpStream, usize) + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (r2, s2) = (requests.clone(), stop.clone());
        let respond = Arc::new(respond);
        std::thread::spawn(move || {
            let n = AtomicUsize::new(0);
            while !s2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        s.set_nonblocking(false).unwrap();
                        let (r3, resp) = (r2.clone(), respond.clone());
                        let idx = n.fetch_add(1, Ordering::SeqCst);
                        std::thread::spawn(move || {
                            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
                            let mut head = Vec::new();
                            let mut buf = [0u8; 4096];
                            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                                match s.read(&mut buf) {
                                    Ok(0) | Err(_) => break,
                                    Ok(k) => head.extend_from_slice(&buf[..k]),
                                }
                            }
                            r3.lock().unwrap().push(head);
                            resp(&mut s, idx);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Server {
            port,
            requests,
            stop,
        }
    }
}

#[tokio::test]
async fn explore_fetch_latest_bytes() {
    let s = Server::start(|st, _| {
        let body =
            r#"{"tag_name":"v9.9.9","html_url":"https://github.com/x/y/releases/tag/v9.9.9"}"#;
        let _ = write!(
            st,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
    });
    let url = format!("http://127.0.0.1:{}/repos/a/b/releases/latest", s.port);
    let r = coin_heaven_lib::commands::update::fetch_latest(&url, "0.1.0").await;
    println!("{r:?}");
    for q in s.requests.lock().unwrap().iter() {
        println!("---\n{}", String::from_utf8_lossy(q));
    }
}

#[tokio::test]
async fn explore_stream_cap() {
    let written = Arc::new(AtomicUsize::new(0));
    let w2 = written.clone();
    let s = Server::start(move |st, _| {
        let _ = write!(
            st,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n"
        );
        let chunk = vec![b'a'; 64 * 1024];
        for _ in 0..1024 {
            match st.write_all(&chunk) {
                Ok(_) => {
                    w2.fetch_add(chunk.len(), Ordering::SeqCst);
                }
                Err(_) => break,
            }
        }
    });
    let url = format!("http://127.0.0.1:{}/x", s.port);
    let t = Instant::now();
    let r = coin_heaven_lib::commands::update::fetch_latest(&url, "0.1.0").await;
    println!("{r:?} in {:?}", t.elapsed());
    std::thread::sleep(Duration::from_millis(300));
    println!(
        "server wrote {} MiB of 64",
        written.load(Ordering::SeqCst) / (1024 * 1024)
    );
}

#[tokio::test]
async fn explore_declared_len() {
    let s = Server::start(|st, _| {
        let _ = write!(
            st,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 999999999\r\nConnection: close\r\n\r\n{{}}"
        );
        std::thread::sleep(Duration::from_secs(4));
    });
    let url = format!("http://127.0.0.1:{}/x", s.port);
    let t = Instant::now();
    let r = coin_heaven_lib::commands::update::fetch_latest(&url, "0.1.0").await;
    println!("{r:?} in {:?}", t.elapsed());
}
