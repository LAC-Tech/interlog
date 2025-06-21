use core::convert;
pub struct Res<FD> {
    pub rc: FD,
    pub usr_data: u64,
}

pub trait ReqFactory {
    type FD: Copy
        + Default
        + Eq
        // So I can unwrap
        + convert::TryInto<usize, Error: core::fmt::Debug>;
    type Req: Copy + Default;
    fn accept_multishot(usr_data: u64, fd: Self::FD) -> Self::Req;
    fn recv(usr_data: u64, fd: Self::FD, buf: &mut [u8]) -> Self::Req;
    fn send(usr_data: u64, fd: Self::FD, buf: &[u8]) -> Self::Req;
}

pub trait AsyncIO<RF: ReqFactory> {
    /// Non blocking
    fn submit(&self, reqs: &[RF::Req]) -> usize;

    /// Blocking
    fn wait_for_res(&self) -> Res<RF::FD>;
}
