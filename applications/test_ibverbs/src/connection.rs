use core::time::Duration;

use alloc::{string::String, vec::Vec};
use ibverbs::{ibv_qp_type::{IBV_QPT_RC, IBV_QPT_UC}, ibv_wc, CompletionQueue, LocalMemoryRegion, ProtectionDomain, QueuePair, QueuePairEndpoint};
use time::Instant;

use crate::{crc32, read_int};

const MEMORY_REGION_SIZE: usize = 512 * 1024;
// this can be larger than the MTU
const PACKET_SIZE: usize = 4096;
const QUEUE_SIZE: usize = 100;

fn send_data(qp: &mut QueuePair, cq: &CompletionQueue, mr: &mut LocalMemoryRegion<u8>) {
    println!("I am the sending host");
    let num_packets = MEMORY_REGION_SIZE / PACKET_SIZE;
    let mut completions = [ibv_wc::default(); QUEUE_SIZE];
    let mut pending_completions = 0;
    let mut failed_completions = 0;
    for idx in 0..num_packets {
        let start = PACKET_SIZE * idx;
        let end = start + PACKET_SIZE;
        unsafe { qp.post_send(
            mr, start..end, idx.try_into().unwrap(),
        ) }
            .expect("failed to post the send request");
        pending_completions += 1;
        for completion in cq.poll(&mut completions)
            .expect("failed to poll for completions") {
            if !completion.is_valid() {
                println!("work completion failed: {:?}", completion.error());
                failed_completions += 1;
            }
            pending_completions -= 1;
        }
        // don't overflow the queue
        if pending_completions >= (QUEUE_SIZE * 3) / 4 {
            sleep::sleep(Duration::from_millis(500)).unwrap();
        }
    }
    while pending_completions > 0 {
        for completion in cq.poll(&mut completions)
            .expect("failed to poll for completions") {
            if !completion.is_valid() {
                println!("work completion failed: {:?}", completion.error());
                failed_completions += 1;
            }
            pending_completions -= 1;
        }
    }
    println!("failed completions: {}", failed_completions);
}

fn recv_data(qp: &mut QueuePair, cq: &CompletionQueue, mr: &mut LocalMemoryRegion<u8>) {
    println!("I am the receiving host");
    let num_packets = MEMORY_REGION_SIZE / PACKET_SIZE;
    let mut completions = [ibv_wc::default(); QUEUE_SIZE];
    let mut pending_completions = 0;
    let mut failed_completions = 0;
    for idx in 0..num_packets {
        let start = PACKET_SIZE * idx;
        let end = start + PACKET_SIZE;
        unsafe { qp.post_receive(
            mr, start..end, idx.try_into().unwrap(),
        ) }
            .expect("failed to post the receive request");
        pending_completions += 1;
        for completion in cq.poll(&mut completions)
            .expect("failed to poll for completions") {
            if !completion.is_valid() {
                println!("work completion failed: {:?}", completion.error());
                failed_completions += 1;
            }
            pending_completions -= 1;
        }
        // don't overflow the queue
        if pending_completions >= (QUEUE_SIZE * 3) / 4 {
            sleep::sleep(Duration::from_millis(500)).unwrap();
        }
    }
    while pending_completions > 0 {
        for completion in cq.poll(&mut completions)
            .expect("failed to poll for completions") {
            if !completion.is_valid() {
                println!("work completion failed: {:?}", completion.error());
                failed_completions += 1;
            }
            pending_completions -= 1;
        }
    }
    println!("failed completions: {}", failed_completions);
}


pub(super) fn run_test(pd: ProtectionDomain, cq: CompletionQueue, args: Vec<String>) -> isize {
    assert_eq!(MEMORY_REGION_SIZE % PACKET_SIZE, 0);
    let mut mr = pd.allocate::<u8>(MEMORY_REGION_SIZE)
        .expect("failed to allocate info memory region");
    let qp_type = match args.iter().find(|a| a == &"-r") {
        Some(_) => IBV_QPT_RC,
        None => IBV_QPT_UC,
    };
    println!("Creating {:?} queue pair...", qp_type);
    let pqp = pd.create_qp(&cq, &cq, qp_type)
        .build()
        .expect("failed to create info queue pair");
    println!("Lid: {}, QPN: {}", pqp.endpoint().lid, pqp.endpoint().num);
    let remote_lid: u16 = read_int("enter remote lid: ").try_into().unwrap();
    let remote_qpn = read_int("enter remote qpn: ");
    println!("Connecting...");
    let mut qp = pqp.handshake(QueuePairEndpoint {
        num: remote_qpn, lid: remote_lid, gid: None,
    })
        .expect("handshake failed");

    let is_sender = args.into_iter().find(|a| a == "-s").is_some();
    
    for i in 0..MEMORY_REGION_SIZE {
        if is_sender {
            // fill the memory region with data
            mr[i] = i as u8;
        } else {
            // zero the memory region
            mr[i] = 0;
        }
    }

    let crc = crc32(0, &mr[0..MEMORY_REGION_SIZE]);
    println!("The initial checksum of the data is {crc:x}");

    let start = Instant::now();
    if is_sender {
        send_data(&mut qp, &cq, &mut mr);
    } else {
        recv_data(&mut qp, &cq, &mut mr);
    }
    
    let end = Instant::now();
    let total_data_mib = MEMORY_REGION_SIZE as f64 / 1024.0 / 1024.0;
    let total_data_mb = MEMORY_REGION_SIZE as f64 / 1000.0 / 1000.0;
    let send_total_time = end - start;
    let send_avg_throughput_mib = total_data_mib / send_total_time.as_secs_f64();
    let send_avg_throughput_mb = total_data_mb / send_total_time.as_secs_f64();
    let send_avg_latency = send_total_time.as_millis() / (MEMORY_REGION_SIZE / PACKET_SIZE) as u128;
    let crc = crc32(0, &mr[0..MEMORY_REGION_SIZE]);
    println!("Results:");
    println!("  Total time: {} s", send_total_time.as_secs_f32());
    println!("  Total data: {total_data_mib} MiB ({total_data_mb} MB)");
    println!(
        "  Average send throughput: {} MiB/s ({} MB/s)",
        send_avg_throughput_mib, send_avg_throughput_mb,
    );
    println!("   Average send latency: {send_avg_latency} us");
    println!("  CRC32: {crc:x}");
    0
}
