use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::{cuda_driver_version_major, cuda_driver_version_minor, Nvml};
use pretty_bytes::converter::convert;

fn main() -> Result<(), NvmlError> {
    let nvml = Nvml::init()?;

    let cuda_version = nvml.sys_cuda_driver_version()?;

    // Grabbing the first device in the system, whichever one that is.
    // If you want to ensure you get the same physical device across reboots,
    // get devices via UUID or PCI bus IDs.
    let device = nvml.device_by_index(0)?;

    // Now we can do whatever we want, like getting some data...
    let name = device.name()?;
    let temperature = device.temperature(TemperatureSensor::Gpu)?;
    let mem_info = device.memory_info()?;
    let graphics_clock = device.clock_info(Clock::Graphics)?;
    let mem_clock = device.clock_info(Clock::Memory)?;
    let link_gen = device.current_pcie_link_gen()?;
    let link_speed = device
        .pcie_link_speed()
        .map(u64::from)
        // Convert megabytes to bytes
        .map(|x| x * 1000000)?;
    let link_width = device.current_pcie_link_width()?;
    let max_link_gen = device.max_pcie_link_gen()?;
    let max_link_width = device.max_pcie_link_width()?;
    let max_link_speed = device
        .max_pcie_link_speed()?
        .as_integer()
        .map(u64::from)
        // Convert megabytes to bytes
        .map(|x| x * 1000000);
    let cuda_cores = device.num_cores()?;
    let architecture = device.architecture()?;

    // And we can use that data (here we just print it)
    print!("\n\n");
    println!(
        "Your {name} (architecture: {architecture}, CUDA cores: {cuda_cores}) \
        is currently sitting at {temperature} °C with a graphics clock of \
        {graphics_clock} MHz and a memory clock of {mem_clock} MHz. Memory \
        usage is {used_mem} out of an available {total_mem}. Right now the \
        device is connected via a PCIe gen {link_gen} x{link_width} interface \
        with a transfer rate of {link_speed} per lane; the max your hardware \
        supports is PCIe gen {max_link_gen} x{max_link_width} at a transfer \
        rate of {max_link_speed} per lane.",
        name = name,
        temperature = temperature,
        graphics_clock = graphics_clock,
        mem_clock = mem_clock,
        used_mem = convert(mem_info.used as _),
        total_mem = convert(mem_info.total as _),
        link_gen = link_gen,
        // Convert byte output to transfers/sec
        link_speed = convert(link_speed as _).replace("B", "T") + "/s",
        link_width = link_width,
        max_link_gen = max_link_gen,
        max_link_width = max_link_width,
        cuda_cores = cuda_cores,
        architecture = architecture,
        max_link_speed = max_link_speed
            // Convert byte output to transfers/sec
            .map(|x| convert(x as _).replace("B", "T") + "/s")
            .unwrap_or_else(|| "<unknown>".into()),
    );

    println!();
    if device.is_multi_gpu_board()? {
        println!("This device is on a multi-GPU board.")
    } else {
        println!("This device is not on a multi-GPU board.")
    }

    println!();
    println!(
        "System CUDA version: {}.{}",
        cuda_driver_version_major(cuda_version),
        cuda_driver_version_minor(cuda_version)
    );

    print!("\n\n");
    Ok(())
}

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
