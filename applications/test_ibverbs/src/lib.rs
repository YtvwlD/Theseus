#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

use core::{marker::PhantomData, time::Duration};
use alloc::{string::String, vec::Vec};

use byteorder::BigEndian;
use ibverbs::{
    ibv_qp_type::{IBV_QPT_RC, IBV_QPT_UC}, ibv_wc, ibv_wc_opcode,
    CompletionQueue, LocalMemoryRegion, PreparedQueuePair, QueuePairEndpoint,
    RemoteMemoryRegion,
};
use sleep::sleep;
use time::Instant;
use zerocopy::{U16, U32, U64};

const RDMA_WRITE_TEST_PACKET_SIZE: usize = 1 << 20;
const RDMA_REGION_SIZE: usize = RDMA_WRITE_TEST_PACKET_SIZE * 2;
const RDMA_WRITE_TEST_MSG_COUNT: usize = 390625;
const RDMA_WRITE_TEST_QUEUE_SIZE: usize = 100;

fn crc32_generic(seed: u32, data: &[u8], polynomial: u32) -> u32 {
    let mut crc = seed;
    for idx in 0..data.len() {
        crc ^= u32::from(data[idx]);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (if crc & 1 != 0 { polynomial } else { 0 });
        }
    }
    crc
}

fn crc32(seed: u32, data: &[u8]) -> u32 {
    const POLYNOMIAL: u32 = 0xedb88320;
    assert_eq!(seed, 0);
    crc32_generic(seed, data, POLYNOMIAL)
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
struct ConnectionInformation {
  lid: U16<BigEndian>,
  qpn: U32<BigEndian>,
  rkey: U32<BigEndian>,
  addr: U64<BigEndian>,
}

impl ConnectionInformation {
    fn recv_rdma_write_test(
        &self, pqp: PreparedQueuePair, mr: &mut LocalMemoryRegion<u8>,
    ) {
        let crc = crc32(0, &mr[0..RDMA_WRITE_TEST_PACKET_SIZE]);
        println!("The initial checksum of the data is {crc:x}");

        // TODO: we can't influence the MTU this way
        // What is context.port_attr.active_mtu?
        let _qp = pqp.handshake(QueuePairEndpoint {
            num: self.qpn.get(), lid: self.lid.get(), gid: None,
        })
            .expect("handshake failed");
        for _ in 0..130 {
            sleep(Duration::from_secs(1)).unwrap();
        }
        let crc = crc32(0, &mr[0..RDMA_WRITE_TEST_PACKET_SIZE]);
        println!("The checksum of the data last received is {crc:x}");
    }

    fn send_rdma_write_test(
        &self, pqp: PreparedQueuePair, cq: &CompletionQueue,
        local_mr: &mut LocalMemoryRegion<u8>,
    ) {
        let mut remote_mr = RemoteMemoryRegion {
            addr: self.addr.get(),
            len: RDMA_REGION_SIZE,
            rkey: self.rkey.get(),
            phantom: PhantomData::<u8>,
        };
        // TODO: we can't influence the MTU this way
        // What is context.port_attr.active_mtu?
        let mut qp = pqp.handshake(QueuePairEndpoint {
            num: self.qpn.get(), lid: self.lid.get(), gid: None,
        })
            .expect("handshake failed");
        let mut completions = [ibv_wc::default(); RDMA_WRITE_TEST_QUEUE_SIZE];
        let mut pending_completions = 0;
        let mut msg_count = RDMA_WRITE_TEST_MSG_COUNT;
        for i in 0..RDMA_WRITE_TEST_PACKET_SIZE {
            local_mr[i] = b'x';
        }
        let start = Instant::now();
        while msg_count > 0 {
            let mut batch_size = RDMA_WRITE_TEST_QUEUE_SIZE - pending_completions;
            if batch_size > msg_count {
                batch_size = msg_count;
            }
            unsafe { qp.rdma_write(
                local_mr, 0..batch_size, &mut remote_mr, 0..batch_size as u64, 0,
            ) }.expect("rdma write failed");
            pending_completions += batch_size;
            msg_count -= batch_size;
            for completion in cq.poll(&mut completions)
                .expect("failed to poll for completions") {
                if !completion.is_valid() {
                    panic!("work completion failed: {:?}", completion.error());
                }
                pending_completions -= 1;
            }
        }
        while pending_completions > 0 {
            for completion in cq.poll(&mut completions)
                .expect("failed to poll for completions") {
                if !completion.is_valid() {
                    panic!("work completion failed: {:?}", completion.error());
                }
                pending_completions -= 1;
            }
        }
        let end = Instant::now();
        let total_data: u64 = (
            RDMA_WRITE_TEST_MSG_COUNT * RDMA_WRITE_TEST_PACKET_SIZE
        ).try_into().unwrap();
        let total_data_mib = total_data as f64 / 1024.0 / 1024.0;
        let total_data_mb = total_data as f64 / 1000.0 / 1000.0;
        let send_total_time = end - start;
        let send_avg_throughput_mib = total_data_mib / send_total_time.as_secs_f64();
        let send_avg_throughput_mb = total_data_mb / send_total_time.as_secs_f64();
        let send_avg_latency = send_total_time.as_millis() / RDMA_WRITE_TEST_MSG_COUNT as u128;
        let crc = crc32(0, &local_mr[0..RDMA_WRITE_TEST_PACKET_SIZE]);
        println!("Results:");
        println!("  Total time: {} s", send_total_time.as_secs_f32());
        println!("  Total data: {total_data_mib} MiB ({total_data_mb} MB)");
        println!(
            "  Average send throughput: {} MiB/s ({} MB/s)",
            send_avg_throughput_mib, send_avg_throughput_mb,
        );
        println!("   Average send latency: {send_avg_latency} us");
        println!("   CRC32: {crc:x}");
    }

    fn send(
        &self, pqp: PreparedQueuePair, cq: &CompletionQueue, remote_lid: u16,
        remote_qpn: u32, mr: &mut LocalMemoryRegion<Self>,
    ) -> Self {
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
        let mut completions = [ibv_wc::default()];
        loop {
            unsafe { qp.post_send(mr, 1..2, 0) }
                .expect("failed to send connection information");
            send_count += 1;
            let completion = cq.poll(&mut completions)
                .expect("failed to poll for completions").get(0);
            if let Some(c) = completion {
                if !c.is_valid() {
                    println!("work completion failed: {:?}", c.error());
                    if c.opcode() == ibv_wc_opcode::IBV_WC_RECV {
                        panic!("failed to receive");
                    }
                } else if c.opcode() == ibv_wc_opcode::IBV_WC_RECV {
                    break;
                }
            }
        }
        println!("Received connection information; {:?}", mr[0]);
        println!("Sender received connection information after sending {send_count} times.");
        mr[0]
    }

    fn recv(
        &self, pqp: PreparedQueuePair, cq: &CompletionQueue, remote_lid: u16,
        remote_qpn: u32, mr: &mut LocalMemoryRegion<Self>,
    ) -> Self {
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
        let mut completions = [ibv_wc::default()];
        loop {
            let completion = cq.poll(&mut completions)
                .expect("failed to poll for completions").get(0);
            if let Some(c) = completion {
                if !c.is_valid() {
                    panic!("work completion failed: {:?}", c.error());
                }
                break;
            }
        }
        println!("Received connection info: {:?}", mr[0]);
        // respond with own connection data
        // put our information in the second slot
        mr[1] = *self;
        unsafe { qp.post_send(mr, 1..2, 0) }
            .expect("failed to send own connection info");
        loop {
            let completion = cq.poll(&mut completions)
                .expect("failed to poll for completions").get(0);
            if let Some(c) = completion {
                if !c.is_valid() {
                    panic!("work completion failed: {:?}", c.error());
                }
                break;
            }
        }
        println!("Answered connection info with {self:?}");
        mr[0]
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
        lid: rdma_pqp.endpoint().lid.into(),
        qpn: rdma_pqp.endpoint().num.into(),
        rkey: rdma_mr.remote().rkey.into(),
        addr: rdma_mr.remote().addr.into(),
    };

    if args.into_iter().find(|a| a == "-s").is_some() {
        println!("I am the sending host");
        let remote_con_inf = my_con_inf.recv(
            cx_pqp, &cq, remote_lid, remote_qpn, &mut cx_mr,
        );
        sleep(Duration::from_secs(1)).unwrap();
        remote_con_inf.send_rdma_write_test(rdma_pqp, &cq, &mut rdma_mr);
    } else {
        println!("I am the receiving host");
        let remote_con_inf = my_con_inf.send(
            cx_pqp, &cq, remote_lid, remote_qpn, &mut cx_mr,
        );
        remote_con_inf.recv_rdma_write_test(rdma_pqp, &mut rdma_mr);
    }
    0
}
