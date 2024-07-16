//use libc;
//use signal_hook;

struct EchoOps {
    handle: *mut libc::c_void,
    start: fn(),
    resume: fn(std::sync::Arc<std::sync::Mutex<String>>),
    suspend: fn(std::sync::Arc<std::sync::Mutex<String>>),
    stop: fn(),
}

impl EchoOps {
    fn open(i: usize) -> Result<EchoOps, String> {
        let filename = if i == 1 {
            b"./libecho1.so\0".as_ptr().cast()
        } else if i == 2 {
            b"./libecho2.so\0".as_ptr().cast()
        } else {
            b"./libecho.so\0".as_ptr().cast()
        };
        let flag = libc::RTLD_LOCAL | libc::RTLD_LAZY;
        let sym_start = b"start\0".as_ptr().cast();
        let sym_resume = b"resume\0".as_ptr().cast();
        let sym_suspend = b"suspend\0".as_ptr().cast();
        let sym_stop = b"stop\0".as_ptr().cast();
        unsafe {
            let handle = libc::dlopen(filename, flag);
            if handle.is_null() {
                return Err(String::from("handle is null"));
            }
            eprintln!("handle = {:?}", handle);
            let start = libc::dlsym(handle, sym_start);
            if start.is_null() {
                libc::dlclose(handle);
                return Err(String::from("start is null"));
            }
            let resume = libc::dlsym(handle, sym_resume);
            if resume.is_null() {
                libc::dlclose(handle);
                return Err(String::from("resume is null"));
            }
            let suspend = libc::dlsym(handle, sym_suspend);
            if suspend.is_null() {
                libc::dlclose(handle);
                return Err(String::from("suspend is null"));
            }
            let stop = libc::dlsym(handle, sym_stop);
            if stop.is_null() {
                libc::dlclose(handle);
                return Err(String::from("stop is null"));
            }
            Ok(EchoOps {
                handle: handle,
                start: std::mem::transmute_copy(&start),
                resume: std::mem::transmute_copy(&resume),
                suspend: std::mem::transmute_copy(&suspend),
                stop: std::mem::transmute_copy(&stop),
            })
        }
    }
    fn close(&mut self) {
        let ret = unsafe { libc::dlclose(self.handle) };
        eprintln!("dlclose = {}", ret);
    }
}

fn main() {
    let mut echo_ops = EchoOps::open(1).unwrap();
    (echo_ops.start)();
    let signals = [
        signal_hook::consts::signal::SIGINT,
        signal_hook::consts::signal::SIGUSR1,
        signal_hook::consts::signal::SIGUSR2,
    ];
    let mut signals = signal_hook::iterator::Signals::new(&signals).unwrap();
    let sig_handle = signals.handle();
    for signal in &mut signals {
        match signal {
            signal_hook::consts::signal::SIGINT => {
                eprintln!("SIGINT");
                break;
            }
            signal_hook::consts::signal::SIGUSR1 => {
                eprintln!("SIGUSR1");
                let data = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                (echo_ops.suspend)(data.clone());
                echo_ops.close();
                echo_ops = EchoOps::open(1).unwrap();
                (echo_ops.resume)(data);
            }
            signal_hook::consts::signal::SIGUSR2 => {
                eprintln!("SIGUSR2");
                let data = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                (echo_ops.suspend)(data.clone());
                echo_ops.close();
                echo_ops = EchoOps::open(2).unwrap();
                (echo_ops.resume)(data);
            }
            _ => unreachable!(),
        }
    }
    sig_handle.close();
    (echo_ops.stop)();
}
