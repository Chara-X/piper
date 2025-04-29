use std::{
    collections, io, pin,
    sync::{self, atomic},
    task,
};
/// [piper::pipe]
pub fn pipe(cap: usize) -> (Reader, Writer) {
    let r = Reader(sync::Arc::new(Pipe {
        buffer: sync::Mutex::new(collections::VecDeque::with_capacity(cap)),
        reader: atomic_waker::AtomicWaker::new(),
        writer: atomic_waker::AtomicWaker::new(),
        closed: atomic::AtomicBool::new(false),
    }));
    let w = Writer(r.0.clone());
    (r, w)
}
/// [piper::Reader]
pub struct Reader(sync::Arc<Pipe>);
impl Reader {
    /// [piper::Reader::len]
    pub fn len(&self) -> usize {
        self.0.buffer.lock().unwrap().len()
    }
}
impl futures_io::AsyncRead for Reader {
    fn poll_read(
        self: pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> task::Poll<io::Result<usize>> {
        if self.len() == 0 {
            self.0.reader.register(cx.waker());
            atomic::fence(atomic::Ordering::SeqCst);
            if self.len() == 0 {
                if self.0.closed.load(atomic::Ordering::Relaxed) {
                    return task::Poll::Ready(Ok(0));
                } else {
                    return task::Poll::Pending;
                }
            }
        }
        self.0.reader.take();
        let mut buffer = self.0.buffer.lock().unwrap();
        let n = buffer.len().min(buf.len());
        for (i, val) in buffer.drain(..n).enumerate() {
            buf[i] = val;
        }
        self.0.writer.wake();
        task::Poll::Ready(Ok(n))
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.0.closed.store(true, atomic::Ordering::SeqCst);
        self.0.writer.wake();
    }
}
/// [piper::Writer]
pub struct Writer(sync::Arc<Pipe>);
impl Writer {
    /// [piper::Writer::len]
    pub fn len(&self) -> usize {
        self.0.buffer.lock().unwrap().len()
    }
}
impl futures_io::AsyncWrite for Writer {
    fn poll_write(
        self: pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> task::Poll<io::Result<usize>> {
        if self.0.closed.load(atomic::Ordering::Relaxed) {
            return task::Poll::Ready(Ok(0));
        }
        let capacity = { self.0.buffer.lock().unwrap().capacity() };
        if self.len() == capacity {
            self.0.writer.register(cx.waker());
            atomic::fence(atomic::Ordering::SeqCst);
            if self.len() == capacity {
                if self.0.closed.load(atomic::Ordering::Relaxed) {
                    return task::Poll::Ready(Ok(0));
                } else {
                    return task::Poll::Pending;
                }
            }
        }
        self.0.writer.take();
        let n = buf.len().min(capacity - self.len());
        let mut buffer = self.0.buffer.lock().unwrap();
        for (i, val) in buf.iter().take(n).enumerate() {
            buffer.push_back(*val);
        }
        self.0.reader.wake();
        task::Poll::Ready(Ok(n))
    }
    fn poll_flush(
        self: pin::Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
    ) -> task::Poll<io::Result<()>> {
        task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: pin::Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
    ) -> task::Poll<io::Result<()>> {
        self.0.closed.store(true, atomic::Ordering::SeqCst);
        self.0.reader.wake();
        self.0.writer.wake();
        task::Poll::Ready(Ok(()))
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.0.closed.store(true, atomic::Ordering::SeqCst);
        self.0.reader.wake();
    }
}
struct Pipe {
    buffer: sync::Mutex<collections::VecDeque<u8>>,
    reader: atomic_waker::AtomicWaker,
    writer: atomic_waker::AtomicWaker,
    closed: atomic::AtomicBool,
}
