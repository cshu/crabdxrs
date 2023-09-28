use crabrs::*;
use crabwebrs::*;
//use log::*;
use log::*;
use reqwest::*;
use serde::{Deserialize, Serialize};
use std::time::*;
use std::*;

pub const MAX_BODY_SIZE: u64 = 140_000_000; //actually 150mb, but just to be safe
pub const MAX_FILE_SIZE: u64 = 349_000_000_000; //actually 350gb, but just to be safe

pub fn mk_client(mut access_token: String) -> CustRes<reqwest::blocking::Client> {
    access_token.insert_str(0, "Bearer ");
    let mut headers = header::HeaderMap::new();
    headers.insert("Authorization", access_token.parse()?);
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .default_headers(headers)
        .build()?;
    Ok(client)
}

pub fn get_email(cli: &reqwest::blocking::Client) -> CustRes<String> {
    Ok(get_current_account(cli)?.email)
}

pub fn get_current_account(
    cli: &reqwest::blocking::Client,
) -> CustRes<dropbox_sdk::users::FullAccount> {
    let rebder = cli
        .post("https://api.dropboxapi.com/2/users/get_current_account")
        .body("");
    let bytes = easy_http_bytes(rebder)?;
    let retval: dropbox_sdk::users::FullAccount = serde_json::from_slice(&bytes)?;
    Ok(retval)
}

pub fn upload_regular(
    cli: &reqwest::blocking::Client,
    locpath: &path::Path,
    fpath: &str,
) -> CustRes<dropbox_sdk::files::FileMetadata> {
    let filobj = fs::File::open(locpath)?;
    let flen = filobj.metadata()?.len();
    if flen > MAX_FILE_SIZE {
        return dummy_err("MAX_FILE_SIZE exceeded.");
    }
    if flen > MAX_BODY_SIZE {
        upload_regular_big(cli, filobj, fpath, flen)
    } else {
        upload_regular_small(cli, filobj, fpath)
    }
}
pub fn upload_regular_small(
    cli: &reqwest::blocking::Client,
    filobj: fs::File,
    fpath: &str,
) -> CustRes<dropbox_sdk::files::FileMetadata> {
    let rebder = cli
        .post("https://content.dropboxapi.com/2/files/upload")
        .header(
            "Dropbox-API-Arg",
            serde_json::to_string(&Upload {
                path: fpath,
                mode: "overwrite",
                mute: true,
            })?,
        )
        .header("Content-Type", "application/octet-stream")
        .body(filobj);
    let bytes = easy_http_bytes(rebder)?;
    let retval: dropbox_sdk::files::FileMetadata = serde_json::from_slice(&bytes)?;
    Ok(retval)
}
pub fn upload_regular_big(
    cli: &reqwest::blocking::Client,
    mut filobj: fs::File,
    fpath: &str,
    flen: u64,
) -> CustRes<dropbox_sdk::files::FileMetadata> {
    use io::Read;
    use io::Seek;
    let adapter = filobj.try_clone()?.take(MAX_BODY_SIZE);
    let bodyobj = blocking::Body::new(adapter);
    let rebder = cli
        .post("https://content.dropboxapi.com/2/files/upload_session/start")
        .header("Content-Type", "application/octet-stream")
        .body(bodyobj);
    let bytes = easy_http_bytes(rebder)?;
    let st_result: dropbox_sdk::files::UploadSessionStartResult = serde_json::from_slice(&bytes)?;
    let mut off = MAX_BODY_SIZE;
    while off + MAX_BODY_SIZE < flen {
        filobj.seek(io::SeekFrom::Start(off))?;
        let adapter = filobj.try_clone()?.take(MAX_BODY_SIZE);
        let bodyobj = blocking::Body::new(adapter);
        let rebder = cli
            .post("https://content.dropboxapi.com/2/files/upload_session/append_v2")
            .header("Content-Type", "application/octet-stream")
            .header(
                "Dropbox-API-Arg",
                serde_json::to_string(&USAppend {
                    cursor: USCursor {
                        session_id: &st_result.session_id,
                        offset: off,
                    },
                })?,
            )
            .body(bodyobj);
        let _bytes = easy_http_bytes(rebder)?;
        off += MAX_BODY_SIZE;
    }
    filobj.seek(io::SeekFrom::Start(off))?;
    let adapter = filobj.take(MAX_BODY_SIZE);
    let bodyobj = blocking::Body::new(adapter);
    let rebder = cli
        .post("https://content.dropboxapi.com/2/files/upload_session/finish")
        .header("Content-Type", "application/octet-stream")
        .header(
            "Dropbox-API-Arg",
            serde_json::to_string(&USFinish {
                cursor: USCursor {
                    session_id: &st_result.session_id,
                    offset: off,
                },
                commit: Upload {
                    path: fpath,
                    mode: "overwrite",
                    mute: true,
                },
            })?,
        )
        .body(bodyobj);
    let bytes = easy_http_bytes(rebder)?;
    let retval: dropbox_sdk::files::FileMetadata = serde_json::from_slice(&bytes)?;
    Ok(retval)
}

pub fn delete(
    cli: &reqwest::blocking::Client,
    fpath: &str,
) -> CustRes<dropbox_sdk::files::DeleteResult> {
    let rebder = cli
        .post("https://api.dropboxapi.com/2/files/delete_v2")
        .json(&Delete {
            path: fpath,
            parent_rev: None,
        });
    let bytes = easy_http_bytes(rebder)?;
    let retval: dropbox_sdk::files::DeleteResult = serde_json::from_slice(&bytes)?;
    Ok(retval)
}

pub fn download_ignore_json_header<W: ?Sized>(
    cli: &reqwest::blocking::Client,
    fpath: &str,
    w: &mut W,
) -> CustRes<u64>
where
    W: std::io::Write,
{
    let rebder = cli
        .post("https://content.dropboxapi.com/2/files/download")
        .header(
            "Dropbox-API-Arg",
            serde_json::to_string(&Download { path: fpath })?,
        );
    //.json(&Download{path: fpath});
    let len = easy_http_copy_to(rebder, w)?;
    Ok(len)
}

pub fn list_folder_regular(
    cli: &reqwest::blocking::Client,
) -> CustRes<Vec<dropbox_sdk::files::Metadata>> {
    let lf = ListFolder {
        path: String::new(),
        recursive: true,
        include_deleted: false,
        include_has_explicit_shared_members: false,
        include_mounted_folders: false,
        limit: Some(2000),
        shared_link: None,
        include_non_downloadable_files: false,
    };
    let mut lfr = list_folder(cli, &lf)?;
    let mut retval = lfr.entries;
    while lfr.has_more {
        info!("REQUESTING MORE FILES DUE TO LARGE NUMBER OF ENTRIES...");
        let cursor: String = lfr.cursor;
        lfr = list_folder_continue(cli, &ListFolderContinue { cursor: &cursor })?;
        retval.append(&mut lfr.entries)
    }
    Ok(retval)
}

pub fn list_folder_continue(
    cli: &reqwest::blocking::Client,
    lfc: &ListFolderContinue,
) -> CustRes<dropbox_sdk::files::ListFolderResult> {
    let rebder = cli
        .post("https://api.dropboxapi.com/2/files/list_folder/continue")
        .json(lfc);
    let bytes = easy_http_bytes(rebder)?;
    let retval: dropbox_sdk::files::ListFolderResult = serde_json::from_slice(&bytes)?;
    Ok(retval)
}

pub fn list_folder(
    cli: &reqwest::blocking::Client,
    lf: &ListFolder,
) -> CustRes<dropbox_sdk::files::ListFolderResult> {
    //let l_body = serde_json::to_vec(lf)?;
    let rebder = cli
        .post("https://api.dropboxapi.com/2/files/list_folder")
        //.body(l_body);
        .json(lf);
    let bytes = easy_http_bytes(rebder)?;
    let retval: dropbox_sdk::files::ListFolderResult = serde_json::from_slice(&bytes)?;
    Ok(retval)
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct SharedLink {
    url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    password: String,
}

fn is_true(obj: &bool) -> bool {
    *obj
}
fn ret_true() -> bool {
    true
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct USFinish<'b, 'a> {
    commit: Upload<'b>,
    #[serde(borrow)]
    cursor: USCursor<'a>,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct USCursor<'a> {
    session_id: &'a str,
    offset: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct USAppend<'a> {
    #[serde(borrow)]
    cursor: USCursor<'a>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Upload<'a> {
    path: &'a str,
    mode: &'static str,
    mute: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Delete<'a> {
    path: &'a str,
    parent_rev: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Download<'a> {
    path: &'a str,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ListFolderContinue<'a> {
    cursor: &'a str,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ListFolder {
    path: String,
    //include_property_groups: TemplateFilterBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    shared_link: Option<SharedLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    limit: Option<u32>, // UInt32(min=1, max=2000)?
    //#[serde(skip_serializing_if = "Option::is_none")]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[serde(default)]
    include_deleted: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[serde(default)]
    include_has_explicit_shared_members: bool,
    //include_media_info: bool,
    //#[serde(skip_serializing_if = "Option::is_none")]
    #[serde(skip_serializing_if = "is_true")]
    #[serde(default = "ret_true")]
    include_mounted_folders: bool,
    #[serde(skip_serializing_if = "is_true")]
    #[serde(default = "ret_true")]
    include_non_downloadable_files: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[serde(default)]
    recursive: bool,
}
impl Default for ListFolder {
    fn default() -> Self {
        Self {
            path: "".to_owned(),
            recursive: false,                           //None,
            include_deleted: false,                     //None,
            include_has_explicit_shared_members: false, //None,
            include_mounted_folders: true,              //None,
            limit: None,
            shared_link: None,
            include_non_downloadable_files: true, //None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_list_folder() {
        cerrln!("\n\nTest Serial");
        let mut obj = ListFolder::default();
        eprintln!("{}", serde_json::to_string_pretty(&obj).unwrap());
        obj.recursive = true;
        eprintln!("{}", serde_json::to_string_pretty(&obj).unwrap());
        obj.include_non_downloadable_files = false;
        eprintln!("{}", serde_json::to_string_pretty(&obj).unwrap());
        //assert_eq!(result, 4);
    }
    #[test]
    fn test_deserialize_list_folder() {
        cerrln!("\n\nTest Deserial");
        let lf: ListFolder = serde_json::from_str(r##"{"path":""}"##).unwrap();
        eprintln!("{:?}", lf);
        //assert_eq!(result, 4);
    }
}
