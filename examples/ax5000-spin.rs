//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.
//!
//! Please note this example uses experimental features and should not be used as a reference for
//! other code. It is here (currently) primarily to help develop EtherCrab.

use env_logger::Env;
use ethercrab::{
    DcSync, MainDevice, MainDeviceConfig, PduStorage, RegisterAddress,
    SubDeviceState::SafeOp,
    Timeouts,
    error::Error,
    idn, idn_to_str,
    std::ethercat_now,
    subdevice_group::{CycleInfo, DcConfiguration, TxRxResponse},
};
use futures_lite::StreamExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use ta::Next;
use ta::indicators::ExponentialMovingAverage;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const TICK_INTERVAL: Duration = Duration::from_micros(250);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting Distributed Clocks demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: TICK_INTERVAL,
            state_transition: Duration::from_secs(20),
            pdu: Duration::from_millis(2000),
            ..Timeouts::default()
        },
        MainDeviceConfig {
            dc_static_sync_iterations: 10_000,
            ..MainDeviceConfig::default()
        },
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    #[cfg(target_os = "windows")]
    std::thread::spawn(move || {
        ethercrab::std::tx_rx_task_blocking(
            &interface,
            tx,
            rx,
            ethercrab::std::TxRxTaskConfig { spinloop: false },
        )
        .expect("TX/RX task")
    });
    #[cfg(not(target_os = "windows"))]
    smol::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

    // Wait for TX/RX loop to start
    thread::sleep(Duration::from_millis(200));

    #[cfg(target_os = "linux")]
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Crossplatform(
        thread_priority::ThreadPriorityValue::try_from(48u8).unwrap(),
    ))
    .expect("Main thread prio");

    smol::block_on(async {
        let mut group = maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        // The group will be in PRE-OP at this point

        for mut subdevice in group.iter_mut(&maindevice) {
            if subdevice.name() == "AX5203-0000-0216" {
                let tick_micros: u16 = TICK_INTERVAL.as_micros().try_into().unwrap();
                let device_num = subdevice.configured_address();

                // Configure the SoE equivalent of RxPDO and TxPDO,
                // the Amplifier Telegram (AT) (Drive -> Controller)
                // and Master Data Telegram (MDT) (Controller -> Drive)
                // via their respective IDNs, S-0-0016 for AT and S-0-0024 for MDT
                for drive in 0..=1 {
                    // S-0-0135 (status word) already fixed
                    let at_config: Vec<u16> = vec![4u16, 48u16, idn!(S, 0, 0051), idn!(S, 0, 0189)];
                    let at_config_bytes: Vec<u8> =
                        at_config.iter().flat_map(|&x| x.to_le_bytes()).collect();
                    let at_config_slice: &[u8] = &at_config_bytes;

                    log::info!(
                        "Writing AT configuration for Device {:#06x}, Drive {}",
                        device_num,
                        drive
                    );
                    subdevice
                        .idn_write_data(drive, idn!(S, 0, 0016), &at_config_slice)
                        .await?;

                    // S-0-0134 (control word) already fixed
                    let mdt_config: Vec<u16> = vec![2u16, 48u16, idn!(S, 0, 0047)];
                    let mdt_config_bytes: Vec<u8> =
                        mdt_config.iter().flat_map(|&x| x.to_le_bytes()).collect();
                    dbg!(&mdt_config_bytes);
                    let mdt_config_slice: &[u8] = &mdt_config_bytes;

                    log::info!(
                        "Writing MDT configuration for Device {:#06x}, Drive {}",
                        device_num,
                        drive
                    );
                    subdevice
                        .idn_write_data(drive, idn!(S, 0, 0024), &mdt_config_slice)
                        .await?;
                }

                for drive in 0..=1 {
                    log::info!(
                        "Writing Feature flags for Device {:#06x}, Drive {}",
                        device_num,
                        drive
                    );
                    let feature_flags: u64 = 0xFE7FF90700000000u64.swap_bytes();
                    subdevice
                        .idn_write_data(drive, idn!(P, 0, 0010), feature_flags)
                        .await?;
                }

                // Configure both drives of the AX5203
                // Write tick period in microseconds to IDNs
                // - S-0-0001 (Control unit cycle time (TNcyc))
                // - S-0-0002 (Communication cycle time (tScyc))
                log::info!("Writing TNcyc for Device {:#06x}, Drive {}", device_num, 0);
                subdevice
                    .idn_write_data(0, idn!(S, 0, 0001), tick_micros)
                    .await?;
                log::info!("Writing tScyc for Device {:#06x}, 0 {}", device_num, 0);
                subdevice
                    .idn_write_data(0, idn!(S, 0, 0002), tick_micros)
                    .await?;

                // Configure operation mode for each drive channel
                for drive in 0..=1 {
                    let mode: u16 = 11;
                    log::info!("Writing {mode} to drive {drive}");
                    subdevice
                        .idn_write_data(drive, idn!(S, 0, 0032), mode)
                        .await?;
                }

                // // Read all of the parameters which need to be configured before moving to SAFE-OP from IDN S-0-0018
                // let (max_length, words) = subdevice
                //     .idn_read_data_list(drive, idn!(S, 0, 0018))
                //     .await?;
                // log::info!("Max required list length: {max_length}");
                // for word in words {
                //     log::info!("Need to set {} before moving to SAFE-OP", idn_to_str(word));
                // }
            }

            log::info!("Setting DC Sync0");
            subdevice.set_dc_sync(DcSync::Sync01 {
                sync1_period: Duration::from_nanos(0),
            });
        }

        log::info!("Group has {} SubDevices", group.len());

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        log::info!("Moving into PRE-OP with PDI");

        let group = group.into_pre_op_pdi(&maindevice).await?;

        log::info!("Done. PDI available. Waiting for SubDevices to align");

        for subdevice in group.iter(&maindevice) {
            let io = subdevice.io_raw();

            log::info!(
                "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
                subdevice.configured_address(),
                subdevice.name(),
                io.inputs().len(),
                io.outputs().len()
            );
        }

        let mut now = Instant::now();
        let start = Instant::now();

        // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
        // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
        // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
        // below 100ns (arbitraily chosen value for this demo), we call the sync good enough and
        // exit the loop.
        loop {
            group
                .tx_rx_sync_system_time(&maindevice)
                .await
                .expect("TX/RX");

            let mut max_deviation = 0;

            for (s1, ema) in group.iter(&maindevice).zip(averages.iter_mut()) {
                let diff = match s1
                    .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                    .await
                {
                    Ok(value) =>
                    // The returned value is NOT in two's compliment, rather the upper bit specifies
                    // whether the number in the remaining bits is odd or even, so we convert the
                    // value to `i32` using that logic here.
                    {
                        let flag = 0b1u32 << 31;

                        if value >= flag {
                            // Strip off negative flag bit and negate value as normal
                            -((value & !flag) as i32)
                        } else {
                            value as i32
                        }
                    }
                    Err(Error::WorkingCounter { .. }) => 0,
                    Err(e) => return Err(e),
                };

                let ema_next = ema.next(diff as f64);

                max_deviation = max_deviation.max(ema_next.abs() as u32);
            }

            if now.elapsed() >= Duration::from_millis(1000) {
                now = Instant::now();

                log::info!("--> Max deviation {} ns", max_deviation);

                // Less than 500ns max deviation as an example threshold.
                // <https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585>
                // mentions less than 100us as a good enough value as well.
                if max_deviation < 500 {
                    log::info!("Clocks settled after {} ms", start.elapsed().as_millis());

                    break;
                }
            }

            tick_interval.next().await;
        }

        log::info!("Alignment done");

        // SubDevice clocks are aligned. We can turn DC on now.
        let group = group
            .configure_dc_sync(
                &maindevice,
                DcConfiguration {
                    // Start SYNC0 100ms in the future
                    start_delay: Duration::from_millis(100),
                    // SYNC0 period should be the same as the process data loop in most cases
                    sync0_period: TICK_INTERVAL,
                    // Send process data half way through cycle
                    sync0_shift: TICK_INTERVAL / 2,
                },
            )
            .await?;

        let group = group
            .into_safe_op(&maindevice)
            .await
            .expect("PRE-OP -> SAFE-OP");
        // group.attempt_transition_to(&maindevice, SafeOp).await?;

        // std::thread::sleep(Duration::from_secs(1));

        // for subdevice in group.iter(&maindevice) {
        //     for drive in 0..=1 {
        //         let (_, indices) = subdevice
        //             .idn_read_data_list(drive, idn!(S, 0, 0021))
        //             .await?;

        //         dbg!(&indices);
        //         for idn_index in indices {
        //             log::info!("Still need to set {}", idn_to_str(idn_index));
        //         }

        //         let primary_mode = subdevice.idn_read_data::<u16>(drive, 32u16).await?;
        //         log::info!("{primary_mode}");
        //     }
        // }

        log::info!("SAFE-OP");

        // Request OP state without waiting for all SubDevices to reach it. Allows the immediate
        // start of the process data cycle, which is required when DC sync is used, otherwise
        // SubDevices never reach OP, most often timing out with a SyncManagerWatchdog error.
        let group = group
            .request_into_op(&maindevice)
            .await
            .expect("SAFE-OP -> OP");

        log::info!("OP requested");

        let op_request = Instant::now();

        // Send PDI and check group state until all SubDevices enter OP state. At this point, we can
        // exit this loop and enter the main process data loop that does not have the state check
        // overhead present here.
        loop {
            let now = Instant::now();

            let response @ TxRxResponse {
                working_counter: _wkc,
                extra: CycleInfo {
                    next_cycle_wait, ..
                },
                ..
            } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

            if response.all_op() {
                break;
            }

            smol::Timer::at(now + next_cycle_wait).await;
        }

        log::info!(
            "All SubDevices entered OP in {} us",
            op_request.elapsed().as_micros()
        );

        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
            .expect("Register hook");

        let mut print_tick = Instant::now();

        // Main application process data cycle
        loop {
            let now = Instant::now();

            let response @ TxRxResponse {
                working_counter: _wkc,
                extra:
                    CycleInfo {
                        dc_system_time: _,
                        next_cycle_wait,
                        cycle_start_offset,
                    },
                ..
            } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

            // Debug logging
            {
                let cycle_start_offset = cycle_start_offset.as_nanos() as u64;

                let should_print = print_tick.elapsed() > Duration::from_secs(1);

                if should_print {
                    print_tick = Instant::now();

                    log::info!(
                        "Offset from start of cycle {} ({:0.2} ms), next tick in {:0.3} ms, group status {:?}",
                        cycle_start_offset,
                        (cycle_start_offset as f32) / 1000.0 / 1000.0,
                        (next_cycle_wait.as_nanos() as f32) / 1000.0 / 1000.0,
                        response.group_state()
                    );
                }

                for sd in group.iter(&maindevice) {
                    if matches!(sd.dc_support(), ethercrab::DcSupport::RefOnly) {
                        continue;
                    }

                    let next_dc_sync_start_time = sd
                        .register_read::<u32>(RegisterAddress::DcSyncStartTime)
                        .await
                        .unwrap_or_default();

                    let sd_time_64 = sd
                        .register_read::<u64>(RegisterAddress::DcSystemTime)
                        .await?;
                    let sd_time_32 = sd_time_64 as u32;

                    let next_sync0 = (next_dc_sync_start_time - sd_time_32) as f64 / 1_000_000.;

                    if should_print {
                        log::info!(
                            "{:#06x}, next sync0 in: {} ms, 32b t {}, {}, 64b t {}",
                            sd.configured_address(),
                            next_sync0,
                            sd_time_32,
                            next_dc_sync_start_time,
                            sd_time_64,
                        );
                    }
                }
            }

            for subdevice in group.iter(&maindevice) {
                let mut o = subdevice.outputs_raw_mut();

                
            }

            smol::Timer::at(now + next_cycle_wait).await;

            // Hook signal so we can write CSV data before exiting
            if term.load(Ordering::Relaxed) {
                log::info!("Exiting...");

                break;
            }
        }

        let group = group
            .into_safe_op(&maindevice)
            .await
            .expect("OP -> SAFE-OP");

        log::info!("OP -> SAFE-OP");

        let group = group
            .into_pre_op(&maindevice)
            .await
            .expect("SAFE-OP -> PRE-OP");

        log::info!("SAFE-OP -> PRE-OP");

        let _group = group.into_init(&maindevice).await.expect("PRE-OP -> INIT");

        log::info!("PRE-OP -> INIT, shutdown complete");

        Ok(())
    })
}
