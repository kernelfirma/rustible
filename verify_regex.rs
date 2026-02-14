use regex::escape;

fn main() {
    let patterns = vec!["a.b*", "a+b", "(a|b)", "*?"];
    for p in patterns {
        let escaped = escape(p);
        println!("Original: {}", p);
        println!("Escaped: {}", escaped);
        let converted = escaped.replace("\*", ".*").replace("\?", ".");
        println!("Converted: {}", converted);
        println!("---");
    }
}
