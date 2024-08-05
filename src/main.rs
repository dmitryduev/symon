use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::{cuda_driver_version_major, cuda_driver_version_minor, Device, Nvml};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
struct GpuMetrics {
    #[serde(flatten)]
    metrics: BTreeMap<String, serde_json::Value>,
}

fn get_child_pids(pid: i32) -> Vec<i32> {
    let output = Command::new("pgrep")
        .args(&["-P", &pid.to_string()])
        .output()
        .expect("Failed to execute pgrep");

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn gpu_in_use_by_process(device: &Device, pid: i32) -> bool {
    let our_pids: Vec<i32> = std::iter::once(pid).chain(get_child_pids(pid)).collect();

    let compute_processes = device.running_compute_processes().unwrap_or_default();
    let graphics_processes = device.running_graphics_processes().unwrap_or_default();

    let device_pids: Vec<i32> = compute_processes
        .iter()
        .chain(graphics_processes.iter())
        .map(|p| p.pid as i32)
        .collect();

    our_pids.iter().any(|&p| device_pids.contains(&p))
}

fn sample_metrics_fallback() -> GpuMetrics {
    let mut metrics = BTreeMap::new();
    metrics.insert("gpu.count".to_string(), json!(0));
    GpuMetrics { metrics }
}

fn sample_metrics(nvml: &Nvml, pid: i32, cuda_version: String) -> Result<GpuMetrics, NvmlError> {
    let start_time = Instant::now();
    let mut metrics = BTreeMap::new();

    metrics.insert("cuda_version".to_string(), json!(cuda_version));

    let device_count = nvml.device_count()?;
    metrics.insert("gpu.count".to_string(), json!(device_count));

    for di in 0..device_count {
        let device = nvml.device_by_index(di)?;
        let gpu_in_use = gpu_in_use_by_process(&device, pid);

        let name = device.name()?;
        metrics.insert(format!("gpu.{}.name", di), json!(name));

        let utilization = device.utilization_rates()?;
        metrics.insert(format!("gpu.{}.gpu", di), json!(utilization.gpu));
        metrics.insert(format!("gpu.{}.memory", di), json!(utilization.memory));

        if gpu_in_use {
            metrics.insert(format!("gpu.process.{}.gpu", di), json!(utilization.gpu));
            metrics.insert(
                format!("gpu.process.{}.memory", di),
                json!(utilization.memory),
            );
        }

        let memory_info = device.memory_info()?;
        metrics.insert(format!("gpu.{}.memoryTotal", di), json!(memory_info.total));
        let memory_allocated = (memory_info.used as f64 / memory_info.total as f64) * 100.0;
        metrics.insert(
            format!("gpu.{}.memoryAllocated", di),
            json!(memory_allocated),
        );
        metrics.insert(
            format!("gpu.{}.memoryAllocatedBytes", di),
            json!(memory_info.used),
        );

        if gpu_in_use {
            metrics.insert(
                format!("gpu.process.{}.memoryAllocated", di),
                json!(memory_allocated),
            );
            metrics.insert(
                format!("gpu.process.{}.memoryAllocatedBytes", di),
                json!(memory_info.used),
            );
        }

        let temperature = device.temperature(TemperatureSensor::Gpu)?;
        metrics.insert(format!("gpu.{}.temp", di), json!(temperature));
        if gpu_in_use {
            metrics.insert(format!("gpu.process.{}.temp", di), json!(temperature));
        }

        let power_usage = device.power_usage()? as f64 / 1000.0;
        metrics.insert(format!("gpu.{}.powerWatts", di), json!(power_usage));
        if gpu_in_use {
            metrics.insert(format!("gpu.process.{}.powerWatts", di), json!(power_usage));
        }

        if let Ok(power_limit) = device.enforced_power_limit() {
            let power_limit = power_limit as f64 / 1000.0;
            metrics.insert(
                format!("gpu.{}.enforcedPowerLimitWatts", di),
                json!(power_limit),
            );
            let power_percent = (power_usage / power_limit) * 100.0;
            metrics.insert(format!("gpu.{}.powerPercent", di), json!(power_percent));

            if gpu_in_use {
                metrics.insert(
                    format!("gpu.process.{}.enforcedPowerLimitWatts", di),
                    json!(power_limit),
                );
                metrics.insert(
                    format!("gpu.process.{}.powerPercent", di),
                    json!(power_percent),
                );
            }
        }

        // New metrics
        let graphics_clock = device.clock_info(Clock::Graphics)?;
        metrics.insert(format!("gpu.{}.graphicsClock", di), json!(graphics_clock));

        let mem_clock = device.clock_info(Clock::Memory)?;
        metrics.insert(format!("gpu.{}.memoryClock", di), json!(mem_clock));

        let link_gen = device.current_pcie_link_gen()?;
        metrics.insert(format!("gpu.{}.pcieLinkGen", di), json!(link_gen));

        if let Ok(link_speed) = device.pcie_link_speed().map(u64::from).map(|x| x * 1000000) {
            metrics.insert(format!("gpu.{}.pcieLinkSpeed", di), json!(link_speed));
        }

        let link_width = device.current_pcie_link_width()?;
        metrics.insert(format!("gpu.{}.pcieLinkWidth", di), json!(link_width));

        let max_link_gen = device.max_pcie_link_gen()?;
        metrics.insert(format!("gpu.{}.maxPcieLinkGen", di), json!(max_link_gen));

        let max_link_width = device.max_pcie_link_width()?;
        metrics.insert(
            format!("gpu.{}.maxPcieLinkWidth", di),
            json!(max_link_width),
        );

        let cuda_cores = device.num_cores()?;
        metrics.insert(format!("gpu.{}.cudaCores", di), json!(cuda_cores));

        let architecture = device.architecture()?;
        metrics.insert(
            format!("gpu.{}.architecture", di),
            json!(format!("{:?}", architecture)),
        );
    }

    let sampling_duration = start_time.elapsed();
    metrics.insert(
        "_sampling_duration_ms".to_string(),
        json!(sampling_duration.as_millis()),
    );

    Ok(GpuMetrics { metrics })
}

fn main() {
    let program_start = Instant::now();

    let nvml_init_start = Instant::now();
    let nvml_result = nvml_wrapper::Nvml::init();
    let nvml_init_duration = nvml_init_start.elapsed();

    println!(
        "NVML initialization time: {} ms",
        nvml_init_duration.as_millis()
    );
    println!(
        "Total startup time: {} ms",
        program_start.elapsed().as_millis()
    );

    let pid = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0".to_string())
        .parse()
        .unwrap_or(0);

    loop {
        let sampling_start = Instant::now();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        let mut gpu_metrics = match &nvml_result {
            Ok(nvml) => match nvml.sys_cuda_driver_version() {
                Ok(cuda_version) => {
                    let cuda_version = format!(
                        "{}.{}",
                        nvml_wrapper::cuda_driver_version_major(cuda_version),
                        nvml_wrapper::cuda_driver_version_minor(cuda_version)
                    );
                    match sample_metrics(nvml, pid, cuda_version) {
                        Ok(metrics) => metrics,
                        Err(_) => sample_metrics_fallback(),
                    }
                }
                Err(_) => sample_metrics_fallback(),
            },
            Err(_) => sample_metrics_fallback(),
        };

        gpu_metrics
            .metrics
            .insert("_timestamp".to_string(), json!(timestamp));
        let serialization_start = Instant::now();
        let json_output = serde_json::to_string(&gpu_metrics.metrics).unwrap();
        let serialization_duration = serialization_start.elapsed();
        println!("{}", json_output);

        let loop_duration = sampling_start.elapsed();
        println!("Total loop time: {} ms", loop_duration.as_millis());
        println!(
            "Serialization time: {} ms",
            serialization_duration.as_millis()
        );
        if loop_duration < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_secs(1) - loop_duration);
        }
    }
}

// use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
// use nvml_wrapper::error::NvmlError;
// use nvml_wrapper::{cuda_driver_version_major, cuda_driver_version_minor, Nvml};
// use pretty_bytes::converter::convert;

// fn main() -> Result<(), NvmlError> {
//     let nvml = Nvml::init()?;

//     let cuda_version = nvml.sys_cuda_driver_version()?;

//     // Grabbing the first device in the system, whichever one that is.
//     // If you want to ensure you get the same physical device across reboots,
//     // get devices via UUID or PCI bus IDs.
//     let device = nvml.device_by_index(0)?;

//     // Now we can do whatever we want, like getting some data...
//     let name = device.name()?;
//     let temperature = device.temperature(TemperatureSensor::Gpu)?;
//     let mem_info = device.memory_info()?;
//     let graphics_clock = device.clock_info(Clock::Graphics)?;
//     let mem_clock = device.clock_info(Clock::Memory)?;
//     let link_gen = device.current_pcie_link_gen()?;
//     let link_speed = device
//         .pcie_link_speed()
//         .map(u64::from)
//         // Convert megabytes to bytes
//         .map(|x| x * 1000000)?;
//     let link_width = device.current_pcie_link_width()?;
//     let max_link_gen = device.max_pcie_link_gen()?;
//     let max_link_width = device.max_pcie_link_width()?;
//     let max_link_speed = device
//         .max_pcie_link_speed()?
//         .as_integer()
//         .map(u64::from)
//         // Convert megabytes to bytes
//         .map(|x| x * 1000000);
//     let cuda_cores = device.num_cores()?;
//     let architecture = device.architecture()?;

//     // And we can use that data (here we just print it)
//     print!("\n\n");
//     println!(
//         "Your {name} (architecture: {architecture}, CUDA cores: {cuda_cores}) \
//         is currently sitting at {temperature} °C with a graphics clock of \
//         {graphics_clock} MHz and a memory clock of {mem_clock} MHz. Memory \
//         usage is {used_mem} out of an available {total_mem}. Right now the \
//         device is connected via a PCIe gen {link_gen} x{link_width} interface \
//         with a transfer rate of {link_speed} per lane; the max your hardware \
//         supports is PCIe gen {max_link_gen} x{max_link_width} at a transfer \
//         rate of {max_link_speed} per lane.",
//         name = name,
//         temperature = temperature,
//         graphics_clock = graphics_clock,
//         mem_clock = mem_clock,
//         used_mem = convert(mem_info.used as _),
//         total_mem = convert(mem_info.total as _),
//         link_gen = link_gen,
//         // Convert byte output to transfers/sec
//         link_speed = convert(link_speed as _).replace("B", "T") + "/s",
//         link_width = link_width,
//         max_link_gen = max_link_gen,
//         max_link_width = max_link_width,
//         cuda_cores = cuda_cores,
//         architecture = architecture,
//         max_link_speed = max_link_speed
//             // Convert byte output to transfers/sec
//             .map(|x| convert(x as _).replace("B", "T") + "/s")
//             .unwrap_or_else(|| "<unknown>".into()),
//     );

//     println!();
//     if device.is_multi_gpu_board()? {
//         println!("This device is on a multi-GPU board.")
//     } else {
//         println!("This device is not on a multi-GPU board.")
//     }

//     println!();
//     println!(
//         "System CUDA version: {}.{}",
//         cuda_driver_version_major(cuda_version),
//         cuda_driver_version_minor(cuda_version)
//     );

//     print!("\n\n");
//     Ok(())
// }

// use nvml_wrapper::Nvml;

// fn main() {
//     let nvml = Nvml::init()?;
//     // Get the first `Device` (GPU) in the system
//     let device = nvml.device_by_index(0)?;

//     let brand = device.brand()?; // GeForce on my system
//     let fan_speed = device.fan_speed(0)?; // Currently 17% on my system
//     let power_limit = device.enforced_power_limit()?; // 275k milliwatts on my system
//     let encoder_util = device.encoder_utilization()?; // Currently 0 on my system; Not encoding anything
//     let memory_info = device.memory_info()?; // Currently 1.63/6.37 GB used on my system
// }

// use std::env;
// use std::net::TcpStream;
// use std::sync::{Arc, Mutex};
// use std::thread;
// use std::time::Duration;

// fn receive_message(stream: Arc<Mutex<TcpStream>>) {
//     loop {
//         // sleep for 1 second, then just print something for now
//         thread::sleep(Duration::from_secs(1));
//         println!("Hello from receive_message");
//     }
// }

// fn main() {
//     let args: Vec<String> = env::args().collect();

//     let port: u16 = args[1].parse().expect("Port must be a number");
//     println!("{}", port);

//     let stream = TcpStream::connect(("localhost", port)).unwrap();
//     let stream = Arc::new(Mutex::new(stream));
//     let stream_clone = stream.clone();

//     let rx = thread::spawn(move || {
//         receive_message(stream_clone);
//     });

//     thread::sleep(Duration::from_secs(5));
// }
