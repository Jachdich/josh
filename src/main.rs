use std::io::Write;
use std::io::BufRead;
use std::env;
extern crate dirs;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use termion;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::event::Event;
use termion::event::Key;

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
        vars.insert("PS1".to_string(), r#"> "#.to_string());
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
        let fmt_string = self.vars.get("PS1").unwrap();
        format!("{}", fmt_string
            .replace("\\w", &std::env::current_dir().unwrap().to_str().unwrap().replace(dirs::home_dir().unwrap().to_str().unwrap(), "~"))
            .replace("\\h", &whoami::hostname())
            .replace("\\u", &whoami::username())
        )
    }

    fn execute_command_get_output(&mut self, command: &str, argv: &[String]) -> String {
        match command {
            "alias" | "cd" => {
                "".to_string();
            },

            command => {
                let res = std::process::Command::new(command)
                    .args(argv)
                    .stdout(std::process::Stdio::piped())
                    .output();
                match res {
                    Ok(mut child) => {
                        return String::from_utf8_lossy(&child.stdout).to_string();
                    }
                    Err(error) => {
                        eprintln!("josh: {}: command not found", command);
                    }
                }
            },
        }
        "".to_string()
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

    fn exec_rc(&mut self) {
        let file = std::fs::File::open(&self.rc_path).unwrap();
        let reader = std::io::BufReader::new(file);
    
        for line in reader.lines() {
            let mut input = line.unwrap();
            if input == "" {
                continue;
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
    
    fn run(&mut self) {
        self.exec_rc();
        let mut hist: Vec<String> = Vec::new();
        loop {
            print!("{}", self.get_ps1());
            std::io::stdout().flush();

            let mut stdout = std::io::stdout().into_raw_mode().unwrap();
            let mut input = String::new();
            let mut hist_pos: usize = hist.len();
            let mut inp_buffer = String::new();
            let mut inp_pos: usize = 0;
            for event in std::io::stdin().events() {
                match event.unwrap() {
                    Event::Key(Key::Ctrl('d')) => {
                        println!("\r");
                        return;
                    }
                    Event::Key(Key::Ctrl('c')) => {
                        print!("^C\r\n");
                        input = "\n".to_string();
                        break;
                    }

                    Event::Key(Key::Up) => {
                        if hist_pos > 0 {
                            hist_pos -= 1;
                            input = hist[hist_pos].clone();
                            inp_pos = input.chars().count();
                        }
                    }

                    Event::Key(Key::Left)  => if inp_pos > 0 { inp_pos -= 1 },
                    Event::Key(Key::Right) => if inp_pos < input.chars().count() { inp_pos += 1 },
                    
                    Event::Key(Key::Down) => {
                        if hist_pos + 1 < hist.len() {
                            hist_pos += 1;
                            input = hist[hist_pos].clone();
                            inp_pos = input.chars().count();
                        } else {
                            hist_pos = hist.len();
                            input = inp_buffer.clone();
                            inp_pos = input.chars().count();
                        }
                    }
                    Event::Key(Key::Char('\n')) => {
                        print!("\r\n");
                        input.push('\n');
                        break;
                    }
                    Event::Key(Key::Backspace) => {
                        if input.len() > 0 {
                            input.remove(inp_pos - 1);
                            inp_pos -= 1;
                        }
                        inp_buffer = input.clone();
                    }
                    Event::Key(Key::Char(c)) => {
                        input.insert(inp_pos, c);
                        inp_buffer = input.clone();
                        inp_pos += 1;
                    }
                    _ => ()
                }

                let pos_from_right = (input.chars().count() - inp_pos) as u16;
                print!("{}\r{}{}{}",
                    termion::clear::CurrentLine,
                    self.get_ps1(),
                    input,
                    if pos_from_right > 0 { termion::cursor::Left(pos_from_right).to_string() }
                    else { "".to_string() },
                );
                stdout.flush();
            }
            drop(stdout);
            if input == "" {
                println!("\r");
                return;
            }
            if input.ends_with('\n') {
                input.pop();
                if input.ends_with('\r') {
                    input.pop();
                }
            }
            
            if let Some(cmd) = hist.last() {
                if cmd != &input {
                    hist.push(input.clone());
                }
            } else {
                hist.push(input.clone());
            }

            let argv = self.parse_argv(input);
            match argv {
                Some(argv) => {
                    if argv.len() == 0 { continue; }
                    if argv[0] == "exit" { break; }
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
                        eprintln!("josh: EOF while scanning string literal");
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
                    if pos == data.len() { continue; }
                    match data[pos] {
                        '(' => {
                            pos += 1;
                            let mut command = String::new();
                            let mut opening = 1;
                            let mut closing = 0;
                            while pos < data.len() && opening > closing{
                                if data[pos] == '(' { opening += 1; }
                                if data[pos] == ')' { closing += 1; }
                                command.push(data[pos]);
                                pos += 1;
                            }
                            command.pop(); //the closing )

                            let argv = self.parse_argv(command);
                            match argv {
                                Some(argv) => {
                                    if argv.len() == 0 { continue; }
                                    let output = self.execute_command_get_output(&argv[0], &argv[1..]);
                                    for c in output.chars() {
                                        res.push(c);
                                    }
                                }
                                None => ()
                            }
                        }

                        '{' => {
                            pos += 1;
                            let mut name = String::new();
                            while pos < data.len() && data[pos] != '}' {
                                name.push(data[pos]);
                                pos += 1;
                            }
                            if self.vars.contains_key(&name) {
                                for c in self.vars.get(&name).unwrap().chars() {
                                    res.push(c);
                                }
                            }
                        }

                        _ => {
                            let mut name = String::new();
                            while pos < data.len() && data[pos] != ' ' {
                                name.push(data[pos]);
                                pos += 1;
                            }
                            if self.vars.contains_key(&name) {
                                for c in self.vars.get(&name).unwrap().chars() {
                                    res.push(c);
                                }
                            }
                        }
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
