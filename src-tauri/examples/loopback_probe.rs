//! Bounded transport diagnostic: no app database, remote host or configuration.
//! cargo run --example loopback_probe -- http|async-tcp|blocking-tcp
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    },
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(3);

fn main() {
    let mode = std::env::args()
        .nth(1)
        .expect("http|async-tcp|blocking-tcp");
    assert!(["http", "async-tcp", "blocking-tcp"].contains(&mode.as_str()));
    let mut failures = 0;
    for round in 0..4 {
        let barrier = Arc::new(Barrier::new(8));
        let workers = (0..8).map(|worker| {
            let mode = mode.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                listener.set_nonblocking(true).unwrap();
                let address = listener.local_addr().unwrap();
                let accepted = Arc::new(AtomicUsize::new(0));
                let accepted_server = accepted.clone();
                let server = std::thread::spawn(move || {
                    let end = Instant::now() + Duration::from_secs(5);
                    while Instant::now() < end {
                        match listener.accept() {
                            Ok((mut socket, _)) => {
                                accepted_server.fetch_add(1, Ordering::SeqCst);
                                socket.set_nonblocking(false).unwrap();
                                socket.set_read_timeout(Some(DEADLINE)).unwrap();
                                socket.set_write_timeout(Some(DEADLINE)).unwrap();
                                let mut bytes = [0; 512];
                                let mut request = Vec::new();
                                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                                    match socket.read(&mut bytes) {
                                        Ok(0) | Err(_) => return,
                                        Ok(count) => request.extend_from_slice(&bytes[..count]),
                                    }
                                    if request.len() > 4096 { panic!("diagnostic request exceeded header bound"); }
                                }
                                let _ = socket.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                                return;
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
                            Err(e) => panic!("diagnostic listener: {e}"),
                        }
                    }
                });
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                barrier.wait();
                let started = Instant::now();
                let result: Result<(), String> = if mode == "blocking-tcp" {
                    (|| -> std::io::Result<()> {
                        let mut socket = TcpStream::connect_timeout(&address, DEADLINE)?;
                        socket.set_read_timeout(Some(DEADLINE))?;
                        socket.set_write_timeout(Some(DEADLINE))?;
                        socket.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
                        let mut bytes = [0; 16];
                        socket.read_exact(&mut bytes)?;
                        assert_eq!(&bytes, b"HTTP/1.1 204 No ");
                        Ok(())
                    })().map_err(|e| format!("{e:?}"))
                } else {
                    runtime.block_on(async {
                        if mode == "http" {
                            let client = reqwest::Client::builder().no_proxy().connect_timeout(DEADLINE).timeout(DEADLINE).build().unwrap();
                            client.get(format!("http://{address}/")).send().await
                                .map(|r| assert_eq!(r.status(), 204)).map_err(|e| format!("{e:?}"))
                        } else {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            tokio::time::timeout(DEADLINE, async {
                                let mut socket = tokio::net::TcpStream::connect(address).await?;
                                socket.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await?;
                                let mut bytes = [0; 16];
                                socket.read_exact(&mut bytes).await?;
                                assert_eq!(&bytes, b"HTTP/1.1 204 No ");
                                Ok::<_, std::io::Error>(())
                            }).await.map_err(|e| format!("async deadline: {e}"))?.map_err(|e| format!("{e:?}"))
                        }
                    })
                };
                let elapsed = started.elapsed().as_millis();
                server.join().unwrap();
                let failed = result.is_err();
                println!("{}", serde_json::json!({"mode":mode,"round":round,"worker":worker,"accepted":accepted.load(Ordering::SeqCst),"elapsedMs":elapsed,"error":result.err()}));
                failed
            })
        }).collect::<Vec<_>>();
        failures += workers
            .into_iter()
            .map(|worker| worker.join().unwrap_or(true))
            .filter(|failed| *failed)
            .count();
    }
    println!(
        "{}",
        serde_json::json!({"mode":mode,"requests":32,"failures":failures})
    );
    if failures != 0 {
        std::process::exit(1);
    }
}
