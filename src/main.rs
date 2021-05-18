use std::io::Write;
use std::env;
extern crate dirs;
use std::collections::HashMap;

use std::path::{Path, PathBuf};

fn expand_tilde<P: AsRef<Path>>(path_user_input: &P) -> Option<PathBuf> {
    let p = path_user_input.as_ref();
    if !p.starts_with("~") {
        return Some(p.to_path_buf());
    }
    if p == Path::new("~") {
        return dirs::home_dir();
    }
    dirs::home_dir().map(|mut h| {
        if h == Path::new("/") {
            // Corner case: `h` root directory;
            // don't prepend extra `/`, just drop the tilde.
            p.strip_prefix("~").unwrap().to_path_buf()
        } else {
            h.push(p.strip_prefix("~/").unwrap());
            h
        }
    })
}

struct Shell {
    w_dir: PathBuf,
    rc_path: PathBuf,
    hist_path: PathBuf,
    vars: HashMap<String, String>,
}

impl Shell {
    fn new() -> Self {
        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("PS1".to_string(), ">".to_string());
        let w_dir = std::env::current_dir().unwrap();
        let mut rc_path = dirs::home_dir().unwrap();
        let mut hist_path = dirs::home_dir().unwrap();
        rc_path.push(".joshrc");
        hist_path.push(".josh_history");
        Shell {
            w_dir, rc_path, hist_path,
            vars,
        }
    }
    
    fn get_ps1(&self) -> String {
        format!("{}", self.vars.get("PS1").unwrap())
    }

    fn execute_command(&mut self, command: &str, argv: &[String]) -> bool {
        match command {
            "cd" => {
                if argv.len() == 1 {
                    self.w_dir = PathBuf::from(argv[0].clone());
                } else if argv.len() == 0 {
                    self.w_dir = PathBuf::from("~");
                } else {
                    println!("josh: cd: too many arguments");
                    return true;
                }
                
                if let Err(e) = env::set_current_dir(&expand_tilde(&self.w_dir).unwrap()) {
                    match e.raw_os_error() {
                        Some(2) => {
                            println!("josh: cd: {}: No such file or directory", self.w_dir.to_str().unwrap());
                        }
                        Some(_) => {
                            eprintln!("josh: cd: other error: {}", e);
                        }
                        None => ()
                    }
                }
            },
            "alias" => {
                
            },

            "exit" => return false,

            command => {
                let res = std::process::Command::new(command).args(argv).spawn();
                match res {
                    Ok(mut child) => {
                        child.wait();
                    }
                    Err(error) => {
                        println!("josh: {}: command not found", command);
                    }
                }
            },
        }
        true
    }
    
    fn run(&mut self) {
        loop {
            print!("{}", self.get_ps1());
            std::io::stdout().flush();

            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            if input == "" {
                println!();
                return;
            }
            if input.ends_with('\n') {
                input.pop();
                if input.ends_with('\r') {
                    input.pop();
                }
            }

            let argv = self.parse_argv(input);
            match argv {
                Some(argv) => {
                    if argv.len() == 0 { continue; }
                    if !self.execute_command(&argv[0], &argv[1..]) {
                        break;
                    }
                }
                None => {
                }
            }
            
        }
    }

    fn eval_vars(&mut self, data: Vec<String>) -> Vec<String> {
        let assign_regex = regex::Regex::new(r#"^\w+=.+"#).unwrap();
        let mut res: Vec<String> = Vec::new();
        for arg in data {
            if assign_regex.is_match(&arg) {
                let (name, value) = &arg.split_once('=').unwrap();
                self.vars.insert(name.to_string(), value.to_string());
            } else {
                res.push(arg);
            }
        }
        res
    }

    fn split_with_strings(&mut self, data: Vec<char>) -> std::option::Option<Vec<String>> {
        let mut res: Vec<String> = Vec::new();
        let mut pos: usize = 0;
        res.push("".to_string());
        while pos < data.len() {
            match data[pos] {
                ' ' => {
                    res.push("".to_string());
                    while pos < data.len() && data[pos] == ' ' {
                        pos += 1;
                    }
                }
                '"' /*"*/ => {
                    pos += 1;
                    while pos < data.len() && data[pos] != '"' /*"*/ {
                        res.last_mut().unwrap().push(data[pos]);
                        pos += 1;
                    }
                    if pos == data.len() {
                        println!("josh: EOF while scanning string literal");
                        return None;
                    }
                    pos += 1;
                }
                c => {
                    res.last_mut().unwrap().push(c);
                    pos += 1;
                }
            }
        }
        if res.last().unwrap() == "" {
            res.pop();
        }
        Some(self.eval_vars(res))
    }
    
    fn parse_argv(&mut self, total: String) -> std::option::Option<Vec<String>> {
        let mut res: Vec<char> = Vec::new();
        let mut pos: usize = 0;
        let data: Vec<char> = total.chars().into_iter().collect();
        while pos < data.len() {
            match data[pos] {
                '$' => {
                    pos += 1;
                    let mut name = String::new();
                    while pos < data.len() && data[pos] != ' ' {
                        name.push(data[pos]);
                        pos += 1;
                    }
                    if self.vars.contains_key(&name) {
                        for c in self.vars.get(&name).unwrap().chars() {
                            res.push(c);
                        }
                    } else {
                        //do nothing so far
                    }
                }
                '\n' => pos += 1,
                c => {
                    res.push(c);
                    pos += 1;
                }
            }
        }
        self.split_with_strings(res)
    }
}

fn main() {
    Shell::new().run();
}
