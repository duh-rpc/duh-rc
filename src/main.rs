//! This example runs a server that responds to any request with "Hello, world!"

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response};
use hyper::body::Incoming as IncomingBody;
use hyper::server::conn::http1;
use hyper::service::Service;
use log::LevelFilter;
use pin_project_lite::pin_project;
use tokio::net::TcpListener;
use tokio::pin;

type ResponseBody = Full<Bytes>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:8000";

    env_logger::Builder::from_default_env()
        .filter_level(LevelFilter::Info)
        .format_target(false)
        .init();

    // TODO: How to implement TLS
    let listener = TcpListener::bind(addr).await?;
    log::info!("listening on http://{addr}");

    loop {
        let (tcp, client_addr) = listener.accept().await?;
        log::info!("New Connection from {}", client_addr);

        let io = TokioIo::new(tcp);

        tokio::task::spawn(async move {
            let srv = Svc {};
            let conn = http1::Builder::new().serve_connection(io, srv);
            // TODO: Figure out why this needs to be pinned.
            pin!(conn);

            tokio::select! {
                res = conn.as_mut() => {
                    // Polling the connection returned a result.
                    // In this case print either the successful or error result for the connection
                    // and break out of the loop.
                    match res {
                        Ok(()) => log::info!("after polling conn, no error"),
                        Err(e) =>  log::warn!("while serving connection: {:?}", e),
                    };
                    return
                }
                _ = tokio::time::sleep(Duration::from_secs(3)) => {
                    // tokio::time::sleep returned a result.
                    // Call graceful_shutdown on the connection and continue the loop.
                   log::warn!("timeout: calling conn.graceful_shutdown");
                    conn.as_mut().graceful_shutdown();
                }
            }
        });
    }
    // TODO: Wait for all spawned tasks to complete before exit
    //Ok(())
}


#[derive(Clone)]
struct Svc {
    //counter: Arc<Mutex<i32>>,
}

impl Service<Request<IncomingBody>> for Svc {
    type Response = Response<ResponseBody>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output=Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<IncomingBody>) -> Self::Future {
        // if req.uri().path() != "/favicon.ico" {
        //     *self.counter.lock().expect("lock poisoned") += 1;
        // }

        let res = match req.uri().path() {
            "/" => mk_response("hello world".into()),
            //"/posts" => mk_response(format!("posts, of course! counter = {:?}", self.counter)),
            /*"/authors" => mk_response(format!(
                "authors extraordinare! counter = {:?}",
                self.counter
            )),*/
            _ => mk_response("oh no! not found".into()),
        };

        Box::pin(async { res })
    }
}

fn mk_response(s: String) -> Result<Response<ResponseBody>, hyper::Error> {
    Ok(Response::builder().body(ResponseBody::new(Bytes::from(s))).unwrap())
}

pin_project! {
    #[derive(Debug)]
    pub struct TokioIo<T> {
        #[pin]
        inner: T,
    }
}

impl<T> TokioIo<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn inner(self) -> T {
        self.inner
    }
}

impl<T> hyper::rt::Read for TokioIo<T>
    where
        T: tokio::io::AsyncRead,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, mut buf: hyper::rt::ReadBufCursor<'_>) -> Poll<Result<(), std::io::Error>> {
        let n = unsafe {
            let mut tbuf = tokio::io::ReadBuf::uninit(buf.as_mut());
            match tokio::io::AsyncRead::poll_read(self.project().inner, cx, &mut tbuf) {
                Poll::Ready(Ok(())) => tbuf.filled().len(),
                other => return other,
            }
        };

        unsafe {
            buf.advance(n);
        }
        Poll::Ready(Ok(()))
    }
}

impl<T> hyper::rt::Write for TokioIo<T>
    where
        T: tokio::io::AsyncWrite,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, std::io::Error>> {
        tokio::io::AsyncWrite::poll_write(self.project().inner, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_flush(self.project().inner, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(self.project().inner, cx)
    }

    fn is_write_vectored(&self) -> bool {
        tokio::io::AsyncWrite::is_write_vectored(&self.inner)
    }

    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[std::io::IoSlice<'_>]) -> Poll<Result<usize, std::io::Error>> {
        tokio::io::AsyncWrite::poll_write_vectored(self.project().inner, cx, bufs)
    }
}

impl<T> tokio::io::AsyncRead for TokioIo<T>
    where
        T: hyper::rt::Read,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, tbuf: &mut tokio::io::ReadBuf<'_>) -> Poll<Result<(), std::io::Error>> {
        let filled = tbuf.filled().len();
        let sub_filled = unsafe {
            let mut buf = hyper::rt::ReadBuf::uninit(tbuf.unfilled_mut());

            match hyper::rt::Read::poll_read(self.project().inner, cx, buf.unfilled()) {
                Poll::Ready(Ok(())) => buf.filled().len(),
                other => return other,
            }
        };

        let n_filled = filled + sub_filled;
        // At least sub_filled bytes had to have been initialized.
        let n_init = sub_filled;
        unsafe {
            tbuf.assume_init(n_init);
            tbuf.set_filled(n_filled);
        }

        Poll::Ready(Ok(()))
    }
}

impl<T> tokio::io::AsyncWrite for TokioIo<T>
    where
        T: hyper::rt::Write,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, std::io::Error>> {
        hyper::rt::Write::poll_write(self.project().inner, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        hyper::rt::Write::poll_flush(self.project().inner, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        hyper::rt::Write::poll_shutdown(self.project().inner, cx)
    }

    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[std::io::IoSlice<'_>]) -> Poll<Result<usize, std::io::Error>> {
        hyper::rt::Write::poll_write_vectored(self.project().inner, cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        hyper::rt::Write::is_write_vectored(&self.inner)
    }
}
