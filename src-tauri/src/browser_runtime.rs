use std::path::PathBuf;
use std::process::{Command,Stdio};
use std::time::{Duration,Instant};

pub fn directory()->Result<PathBuf,String>{
    if cfg!(debug_assertions){return Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime"));}
    Ok(std::env::current_exe().map_err(|e|e.to_string())?.parent().ok_or("Missing install directory")?.join("runtime"))
}

#[tauri::command]
pub async fn setup_firefox_collector()->Result<String,String>{
    tauri::async_runtime::spawn_blocking(||{
        let runtime=directory()?;
        if !runtime.join("node.exe").is_file(){return Err("Bundled browser runtime missing; repair the Masar installation".into());}
        let mut command=Command::new(runtime.join("node.exe"));
        command.arg(runtime.join("node_modules/playwright/cli.js")).args(["install","firefox"])
            .current_dir(&runtime).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        let mut child=command.spawn().map_err(|e|format!("Firefox setup could not start: {e}"))?;
        let started=Instant::now();
        loop {
            match child.try_wait(){
                Ok(Some(status))=>return if status.success(){Ok("Firefox installed. Run a collector to verify browser launch.".into())}else{Err("Firefox installation failed. Check internet access and retry setup.".into())},
                Err(e)=>{let _=child.kill();let _=child.wait();return Err(e.to_string());},
                Ok(None)=>{}
            }
            if started.elapsed()>Duration::from_secs(300){
                #[cfg(windows)] {use std::os::windows::process::CommandExt;let _=Command::new("taskkill.exe").args(["/PID",&child.id().to_string(),"/T","/F"]).creation_flags(0x08000000).stdout(Stdio::null()).stderr(Stdio::null()).status();}
                let _=child.kill();let _=child.wait();return Err("Firefox installation timed out; retry setup".into());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }).await.map_err(|e|e.to_string())?
}
