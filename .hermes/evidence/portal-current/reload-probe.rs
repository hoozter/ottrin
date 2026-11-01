fn main() {
    match ottrin_platform::portal_integration::apply() {
        Ok(message)=>println!("{message}"),
        Err(error)=>{eprintln!("{error}");std::process::exit(2);}
    }
}
