use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

#[derive(Debug, PartialEq, Eq)]
pub struct CapturedRequest {
    pub path: String,
    pub params: BTreeMap<String, Vec<String>>,
}

pub fn spawn_capture_server(response_body: &'static str) -> (String, JoinHandle<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
    let address = listener.local_addr().expect("capture server address");
    let capture = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept captured request");
        let mut reader = BufReader::new(stream.try_clone().expect("clone captured stream"));
        let mut request_line = String::new();
        reader
            .read_line(&mut request_line)
            .expect("read captured request line");
        loop {
            let mut header = String::new();
            reader
                .read_line(&mut header)
                .expect("read captured request header");
            if header == "\r\n" || header.is_empty() {
                break;
            }
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .expect("write capture response");
        parse_request_line(&request_line)
    });
    (format!("http://{address}"), capture)
}

fn parse_request_line(request_line: &str) -> CapturedRequest {
    let target = request_line
        .split_whitespace()
        .nth(1)
        .expect("captured request target");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut params = BTreeMap::<String, Vec<String>>::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        params
            .entry(percent_decode(name))
            .or_default()
            .push(percent_decode(value));
    }
    CapturedRequest {
        path: path.to_string(),
        params,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(b' '),
            b'%' => {
                let high = hex_digit(*bytes.get(index + 1).expect("high percent digit"));
                let low = hex_digit(*bytes.get(index + 2).expect("low percent digit"));
                decoded.push(high * 16 + low);
                index += 2;
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    String::from_utf8(decoded).expect("UTF-8 query parameter")
}

fn hex_digit(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        b'A'..=b'F' => digit - b'A' + 10,
        _ => panic!("invalid percent digit"),
    }
}
