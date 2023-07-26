use std::{
    collections::VecDeque,
    pin::Pin,
    task::{self, Context, Poll},
    io,
};

use libp2p::futures::{AsyncRead, AsyncWrite};

#[derive(Default)]
pub struct Inner {
    command_queue: VecDeque<(usize, Vec<u8>)>,
    handshake: Option<(usize, [u8; 15])>,
    buffer: Buffer,
    direction: bool,
}

struct Buffer {
    offset: usize,
    buf: Vec<u8>,
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer {
            offset: 0,
            buf: vec![0; Self::INITIAL_SIZE],
        }
    }
}

impl Buffer {
    const INITIAL_SIZE: usize = 0x1000;

    pub fn poll_fill<T>(&mut self, cx: &mut Context<'_>, io: &mut T) -> Poll<io::Result<()>>
    where
        T: AsyncRead + Unpin,
    {
        let io = Pin::new(io);
        if self.buf.len() == self.offset {
            self.buf.reserve(self.buf.len());
        }
        let read = task::ready!(io.poll_read(cx, &mut self.buf[self.offset..]))?;
        self.offset += read;
        if read != 0 {
            dbg!(read);
        }
        Poll::Ready(Ok(()))
    }

    pub fn try_cut(&mut self) -> Option<Vec<u8>> {
        if self.offset >= 8 {
            let msg_len = u64::from_le_bytes(self.buf[..8].try_into().unwrap()) as usize;
            if self.offset >= 8 + msg_len {
                self.offset -= 8 + msg_len;
                let yi = self.buf[8..(8 + msg_len)].to_vec();
                self.buf = self.buf[(8 + msg_len)..].to_vec();
                let new_len = self.buf.len().next_power_of_two().max(Self::INITIAL_SIZE);
                self.buf.resize(new_len, 0);
                return Some(yi);
            }
        }

        None
    }
}

impl Inner {
    const HANDSHAKE_MSG: [u8; 15] = *b"\x07\x00\x00\x00\x00\x00\x00\x00\x02\xfdRPC\x00\x01";

    pub fn add(&mut self, command: Vec<u8>) {
        self.command_queue.push_back((0, command));
    }

    pub fn poll<T>(&mut self, cx: &mut Context<'_>, mut io: &mut T) -> Poll<io::Result<Vec<u8>>>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        // do handshake if needed
        match &mut self.handshake {
            None => self.handshake = Some((0, Self::HANDSHAKE_MSG)),
            Some((offset, buf)) => {
                if *offset < buf.len() {
                    let len = task::ready!(Pin::new(&mut io).poll_write(cx, &buf[*offset..]))?;
                    *offset += len;
                }
            }
        }

        self.direction = !self.direction;
        if self.direction {
            match self.poll_send(cx, io) {
                Poll::Ready(r) => Poll::Ready(r),
                Poll::Pending => self.poll_recv(cx, io),
            }
        } else {
            match self.poll_recv(cx, io) {
                Poll::Ready(r) => Poll::Ready(r),
                Poll::Pending => self.poll_send(cx, io),
            }
        }
    }

    pub fn poll_recv<T>(
        &mut self,
        cx: &mut Context<'_>,
        mut io: &mut T,
    ) -> Poll<io::Result<Vec<u8>>>
    where
        T: AsyncRead + Unpin,
    {
        if let Some(bytes) = self.buffer.try_cut() {
            return if bytes == &Self::HANDSHAKE_MSG[8..] {
                Poll::Pending
            } else {
                Poll::Ready(Ok(bytes))
            };
        }
        task::ready!(self.buffer.poll_fill(cx, &mut io))?;

        Poll::Pending
    }

    pub fn poll_send<T>(
        &mut self,
        cx: &mut Context<'_>,
        mut io: &mut T,
    ) -> Poll<io::Result<Vec<u8>>>
    where
        T: AsyncWrite + Unpin,
    {
        if let Some((offset, buf)) = self.command_queue.front_mut() {
            if *offset < buf.len() {
                let written = task::ready!(Pin::new(&mut io).poll_write(cx, &buf[*offset..]))?;
                *offset += written;
                if *offset >= buf.len() {
                    dbg!(*offset);
                    self.command_queue.pop_front();
                }
            }

            self.poll_send(cx, io)
        } else {
            Poll::Pending
        }
    }
}
