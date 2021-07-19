use colored::Colorize;

pub fn connect_failed() {
    eprintln!("{}", "CONNECTION REFUSED".red())
}

pub fn log(request: &httparse::Request, response: &httparse::Response) {
    let out = match response.code {
        Some(code @ 200..=299) => format!("{}", code).green(),
        Some(code) => format!("{}", code).red(),
        _ => "???".red(),
    };

    let method = request.method.unwrap_or("????");
    let path = request.path.unwrap_or("");

    eprint!("{}", out);

    eprintln!("\t\t{}\t{}", method.to_uppercase().yellow(), path.blue());
}
