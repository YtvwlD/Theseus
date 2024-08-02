#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

use alloc::{string::String, vec::Vec};
use ibverbs::{ibv_qp_type::{IBV_QPT_RC, IBV_QPT_UC}, CompletionQueue, MemoryRegion, PreparedQueuePair, QueuePairEndpoint};

const RDMA_WRITE_TEST_PACKET_SIZE: usize = 1 << 20;
const RDMA_REGION_SIZE: usize = RDMA_WRITE_TEST_PACKET_SIZE * 2;

#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
struct ConnectionInformation {
  lid: u16,
  qpn: u32,
  rkey: u32,
  addr: u64,
}

impl ConnectionInformation {
    fn send(&self, pqp: PreparedQueuePair, cq: &CompletionQueue, remote_lid: u16, remote_qpn: u32, mr: &mut MemoryRegion<Self>) -> Self {
        // TODO: we can't influence the MTU this way
        // What is context.port_attr.active_mtu?
        let mut qp = pqp.handshake(QueuePairEndpoint {
            num: remote_qpn, lid: remote_lid, gid: None,
        })
            .expect("handshake failed");
        println!("Sending connection info: {self:?}");
        // Requesting remote connection information is posted once and will
        // eventually be completed. Due to the unreliably connected nature of
        // the QP, sending our own connection information has to be done
        // periodically until it will eventually be acknowledged.
        
        // request remote rdma information
        unsafe { qp.post_receive(
            mr,
            0..1,
            0,
        ) }
            .expect("failed to request remote rdma information");
        // put our information in the second slot
        mr[1] = *self;
        let mut send_count = 0;
        loop {
            unsafe { qp.post_send(mr, 1..2, 0) }
                .expect("failed to send connection information");
            send_count += 1;
            todo!()
        }
        println!("Received connection information; {:?}", mr[0]);
        println!("Sender received connection information after sending {send_count} times.");
        mr[0]
    }

    fn recv(&self, pqp: PreparedQueuePair, cq: &CompletionQueue, remote_lid: u16, remote_qpn: u32, mr: &mut MemoryRegion<Self>) -> Self {
        // TODO: we can't influence the MTU this way
        // What is context.port_attr.active_mtu?
        let mut qp = pqp.handshake(QueuePairEndpoint {
            num: remote_qpn, lid: remote_lid, gid: None,
        })
            .expect("handshake failed");
        // receive remote connection data
        unsafe { qp.post_receive(
            mr,
            0..1,
            0,
        ) }
            .expect("failed to request remote rdma information");
        todo!()
    }
}

fn read_int(prompt: &str) -> u32 {
    println!("{}", prompt);
    let mut buf = [0u8; 10];
    let stdin = app_io::stdin().expect("failed to open stdin");
    stdin.read(&mut buf).expect("failed to read from stdin");
    String::from_utf8(buf.to_vec())
        .expect("failed to parse string")
        .trim_end_matches("\0")
        .trim()
        .parse()
        .expect("not a valid integer")
}

pub fn main(args: Vec<String>) -> isize {
    let context = ibverbs::devices()
        .expect("failed to list devices")
        .iter()
        .next()
        .expect("failed to get device")
        .open()
        .expect("failed to open device");
    let pd = context.alloc_pd()
        .expect("failed to allocate protection domain");
    let mut cx_mr = pd.allocate::<ConnectionInformation>(2)
        .expect("failed to allocate info memory region");
    let cq = context.create_cq(4096, 0)
        .expect("failed to create completion queue");
    println!("Creating queue pair...");
    let cx_pqp = pd.create_qp(&cq, &cq, IBV_QPT_UC)
        .build()
        .expect("failed to create info queue pair");
    println!("Lid: {}, QPN: {}", cx_pqp.endpoint().lid, cx_pqp.endpoint().num);
    let remote_lid: u16 = read_int("enter remote lid: ").try_into().unwrap();
    let remote_qpn = read_int("enter remote qpn: ");

    let rdma_pqp = pd.create_qp(&cq, &cq, IBV_QPT_RC)
        .allow_remote_rw()
        .build()
        .expect("failed to create data queue pair");
    let mut rdma_mr = pd.allocate::<u8>(RDMA_REGION_SIZE)
        .expect("failed to allocate data memory region");
    let my_con_inf = ConnectionInformation {
        lid: rdma_pqp.endpoint().lid.to_be(),
        qpn: rdma_pqp.endpoint().num.to_be(),
        rkey: rdma_mr.rkey().key.to_be(),
        addr: (rdma_mr.as_mut_ptr() as u64).to_be(),
    };

    if args.into_iter().find(|a| a == "-s").is_some() {
        println!("I am the sending host");
        let remote_con_inf = my_con_inf.recv(
            cx_pqp, &cq, remote_lid, remote_qpn, &mut cx_mr,
        );
    } else {
        println!("I am the receiving host");
        let remote_con_inf = my_con_inf.send(
            cx_pqp, &cq, remote_lid, remote_qpn, &mut cx_mr,
        );
    }
    0
}
