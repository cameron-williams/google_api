# google_api

google_api is an extremely basic and not well furnished Google API client library that I wrote for use in other scripts

Currently it only supports Drive with the following features:
- Manages and refreshes authentication with consent screen if required
- Get File(s) and File metadata
- Download files to specific local path
- Upload given file to Drive
- Update file at given Drive url with given file
- Delete file at given Drive url

Although as I start using other APIs or need more Drive functionality I will add it in.

#####----------------------------------------------------------------------------------------------------------

This repo is very much a work in progress, as I'm still new to Rust and there are a lot more future features I would like to add once I get more comfortable with the language.

If you happen to come across this repo and have any tips/pointers/criticism on my code, I would urge you to leave me a message as I know that my code is very basic, and I'm always looking to improve!


## Getting Started

If you really want to use this lib, you have to pull it locally and you can reference it in your Cargo.toml

```
> cd ~Projects
> git clone git@github.com:cameron-williams/google_api.git
> cd my_project

# Add to Cargo.toml:
google_api = { path = "../google_api"}


# you can know use google_api in that project.rs
use google_api::Drive;

let d = Drive::new(CLIENT_ID, CLIENT_SECRET, NONE);
d.pull("https://drive.google.com/open?id=whatever);
```



### Repo

In earlier commits there was a GOOGLE_CLIENT_SECRET value that was commited. It is now invalid so no point in trying it.

## Authors

* **Cameron Williams**  - [Github](https://github.com/cameron-williams)


## License

This project is licensed under the MIT License - see the [LICENSE.md](LICENSE.md) file for details


