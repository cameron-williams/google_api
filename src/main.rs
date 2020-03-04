pub mod lib;
use lib::{Drive};
use std::path::PathBuf;


fn main() {
    let drive = Drive::new(
        String::from("1979642470-56gn3t87ibds6kllqp6rqu09im00qj5i.apps.googleusercontent.com"),
        String::from("PekwZar1ZaqSjWRz7A2PMkaF"),
        None
    ).unwrap();

    // drive.download_file(
    //     "https://drive.google.com/open?id=14dM3nWVnxKahLsUa5ERWfQOoxX7hcXsN",
    //     PathBuf::from("/home/cam/Downloads/test2.pdf")
    // );
    // drive.upload_file(PathBuf::from("/home/cam/Pictures/1023191543a.jpg"));
    // drive.file_metadata("https://drive.google.com/open?id=14dM3nWVnxKahLsUa5ERWfQOoxX7hcXsN");



    drive.delete_file("https://drive.google.com/open?id=1PNOK192cclhR5qL2M7dw6pZa3wCCuoay").unwrap();


    // std::thread::sleep(std::time::Duration::from_secs(100000));
}