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
    aliases: HashMap<String, String>,
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
            vars, aliases: HashMap::new(),
        }
    }

    fn append_history(&self, item: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.hist_path)
            .unwrap();
        writeln!(file, "{}", item).unwrap();
    }

    fn read_history(&self, line_num: usize) -> String {
        if let Ok(file) = std::fs::File::open(&self.hist_path) {
            let content = std::io::BufReader::new(&file);
            let mut lines = content.lines();
            return lines.nth(line_num).unwrap().unwrap();
        } else {
            return "".to_string();
        }
    }

    fn get_hist_len(&self) -> usize {
        let file = std::fs::File::open(&self.hist_path);
        if let Ok(file) = file {
            let reader = std::io::BufReader::new(file);
            let mut count = 0;
            for line in reader.lines() { 
                if line.unwrap() != "" { count += 1; }
            }
            count
        } else {
            0
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
                    Ok(child) => {
                        return String::from_utf8_lossy(&child.stdout).to_string();
                    }
                    Err(_) => {
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
                if argv.len() > 2 {
                    eprintln!("josh: alias: too many arguments");
                } else if argv.len() < 2 {
                    eprintln!("josh: alias: too few arguments");
                } else {
                    self.aliases.insert(argv[0].to_owned(), argv[1].to_owned());
                }
            },

            "exit" => return false,

            command => {
                let actual_command: &str;
                let mut actual_argv: Vec<&String> = argv.iter().collect();
                let alias_argv: Vec<String>;
                if self.aliases.contains_key(command) {
                    alias_argv = self.parse_argv(self.aliases[command].to_owned()).unwrap(); //TODO unwrap bad here
                    actual_command = &alias_argv[0];
                    actual_argv.extend(&alias_argv[1..]);
                } else {
                    actual_command = command;
                }
                let res = std::process::Command::new(actual_command).args(actual_argv).spawn();
                match res {
                    Ok(mut child) => {
                        child.wait().unwrap();
                    }
                    Err(_) => {
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

    fn get_tab_complete(&self, input: &str) -> (Vec<String>, Vec<String>) {
        let argv: Vec<&str> = input.split_whitespace().into_iter().collect();
        if argv.len() == 1 && !input.ends_with(" ") {
            //complete command
            (Vec::new(), Vec::new())
        } else if input.ends_with(" ") {
            //new arg
            let mut res: Vec<String> = Vec::new();
            for path in std::fs::read_dir("./").unwrap() {
                let path = path.unwrap();
                let mut name = path.path().to_str().unwrap()[2..].to_string();
                if path.path().is_dir() {
                    name.push('/');
                }
                res.push(name);
            }
            (res.clone(), res)
        } else {
            //complete current arg
            let mut res: Vec<String> = Vec::new();
            let mut visual: Vec<String> = Vec::new();
            let mut path_to_search: PathBuf;
            if argv[argv.len() - 1].starts_with("/") {
                path_to_search = PathBuf::new();
            } else {
                path_to_search = PathBuf::from("./");
            }
            
            path_to_search.push(PathBuf::from(argv[argv.len() - 1]));
            
            if !std::path::Path::new(&path_to_search).exists() {
                path_to_search.pop();
            }
            
            let paths = std::fs::read_dir(path_to_search);
            if let Ok(paths) = paths {
                for path in paths {
                    let path = path.unwrap().path();
                    let mut item = path.to_str().unwrap();
                    if item.starts_with("./") {
                        item = &item[2..];
                    }
                    if item.starts_with(&argv[argv.len() - 1]) {
                        let mut name_only = Path::new(&item).file_name().unwrap().to_str().unwrap().to_string();
                        let mut full_path = Path::new(&item).to_str().unwrap().to_string();
                        if Path::new(&item).is_dir() {
                            name_only.push('/');
                            full_path.push('/');
                        }
                        visual.push(name_only);
                        res.push(full_path);
                    }
                }
            }
            (res, visual)
        }
        
    }
    
    fn run(&mut self) {
        self.exec_rc();
        loop {
            print!("{}", self.get_ps1());
            std::io::stdout().flush().unwrap();

            let mut stdout = std::io::stdout().into_raw_mode().unwrap();
            
            let mut input = String::new();
            let mut inp_buffer = String::new();
            let mut inp_pos: usize = 0;
            
            let mut hist_len = self.get_hist_len();
            let mut hist_pos: usize = hist_len;

            for event in std::io::stdin().events() {
                let nhist_len = self.get_hist_len();
                if hist_pos == hist_len && hist_len != nhist_len {
                    hist_pos = nhist_len;
                }
                hist_len = nhist_len;
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
                            input = self.read_history(hist_pos);
                            inp_pos = input.chars().count();
                        }
                    }

                    Event::Key(Key::Left)  => if inp_pos > 0 { inp_pos -= 1 },
                    Event::Key(Key::Right) => if inp_pos < input.chars().count() { inp_pos += 1 },
                    
                    Event::Key(Key::Down) => {
                        if hist_pos + 1 < hist_len {
                            hist_pos += 1;
                            input = self.read_history(hist_pos);
                            inp_pos = input.chars().count();
                        } else {
                            hist_pos = hist_len;
                            input = inp_buffer.clone();
                            inp_pos = input.chars().count();
                        }
                    }

                    Event::Key(Key::Char('\t')) => {
                        let results = self.get_tab_complete(&input);
                        if results.0.len() == 1 {
                            let mut argv: Vec<&str> = input.split_whitespace().into_iter().collect();
                            let len = argv.len();
                            argv[len - 1] = &results.0[0];
                            input = argv.join(" ");
                            inp_pos = input.chars().count();
                        } else if results.0.len() > 1 {
                            print!("\n"); //dunno why but this makes the prompt print again like bash lol
                            print!("\r");
                            for n in results.1 {
                                print!("{} ", n);
                            }
                            print!("\n");
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
                stdout.flush().unwrap();
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

            let hlen = self.get_hist_len();
            if hlen == 0 || self.read_history(hlen - 1) != input {
                self.append_history(&input);
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
                            while pos < data.len() && data[pos].is_alphanumeric() {
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
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 {
        if args[1] == "--version" {
            println!("0.1.5");
            return;
        }
    }
    Shell::new().run();
}
