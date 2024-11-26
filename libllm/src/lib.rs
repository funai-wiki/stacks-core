
use std::error;
use std::io::{Error, ErrorKind};
use std::thread;

use serde::{Deserialize, Serialize};

use hex;
use openai::{
    chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole},
    set_base_url, set_key,
};

use stacks_common::util::hash::Sha256Sum;

mod db;

pub const INFER_CHECK_SUCCESS: i32 = 1;
pub const INFER_CHECK_FAIL: i32 = 0;

pub async fn infer(user_input: &str, context_messages: Option<Vec<ChatCompletionMessage>>) -> Result<String, Box<dyn error::Error>> {
    if user_input.is_empty() {
        return Err(Box::new(Error::new(ErrorKind::InvalidInput, "EMPTY_USER_INPUT")));
    }

    let mut messages = Vec::new();
    if let Some(context_messages) = context_messages {
        messages.extend(context_messages);
    }

    messages.push(ChatCompletionMessage {
        role: ChatCompletionMessageRole::User,
        content: Some(user_input.to_string()),
        name: None,
        function_call: None,
    });

    set_key("ollama".to_string());
    set_base_url("http://localhost:11434/v1/".to_string());

    let chat_completion = ChatCompletion::builder("llama3.1", messages.clone())
        .create()
        .await?;

    Ok(chat_completion.choices[0].message.content.clone().expect("ResponseError"))
}

pub async fn infer_check(user_input: &str, output: &str, context_messages: Option<Vec<ChatCompletionMessage>>)  -> Result<i32, Box<dyn error::Error>> {
    if user_input.is_empty() {
        return Err(Box::new(Error::new(ErrorKind::InvalidInput, "EMPTY_USER_INPUT")));
    }

    if output.is_empty() {
        return Err(Box::new(Error::new(ErrorKind::InvalidInput, "EMPTY_OUTPUT")));
    }

    let mut messages = Vec::new();
    if let Some(context_messages) = context_messages {
        messages.extend(context_messages);
    }

    messages.push(ChatCompletionMessage {
        role: ChatCompletionMessageRole::User,
        content: Some(format!("请评估问题和答案是否匹配，匹配：1，不匹配：0。
# 说明
直接提供最终分数，不要解释和说明
# 问题
{user_input}
# 回答
{output}", user_input=user_input, output=output)),
        name: None,
        function_call: None,
    });

    set_key("ollama".to_string());
    set_base_url("http://localhost:11434/v1/".to_string());

    let chat_completion = ChatCompletion::builder("llama3.1", messages.clone())
        .create()
        .await?;

    Ok(chat_completion.choices[0].message.content.clone().expect("ResponseError").parse::<i32>()?)
}


pub async fn random_question() -> Result<String, Box<dyn error::Error>> {
    let mut messages = Vec::new();
    messages.push(ChatCompletionMessage {
        role: ChatCompletionMessageRole::User,
        content: Some("简单直接给我随机出一个问题，文本长度在10-100之间。不需要解释和说明。".to_string()),
        name: None,
        function_call: None,
    });

    set_key("ollama".to_string());
    set_base_url("http://localhost:11434/v1/".to_string());

    let chat_completion = ChatCompletion::builder("llama3.1", messages.clone())
        .temperature(0.9)
        .create()
        .await?;

    Ok(chat_completion.choices[0].message.content.clone().expect("ResponseError"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InferStatus {
    Created = 1,
    InProgress = 2,
    Success = 3,
    Failure = 4,
    NotFound = 5,
}

impl From<u8> for InferStatus {
    fn from(value: u8) -> Self {
        match value {
            1 => InferStatus::Created,
            2 => InferStatus::InProgress,
            3 => InferStatus::Success,
            4 => InferStatus::Failure,
            5 => InferStatus::NotFound,
            _ => panic!("UNKNOWN_VALUE {}", value),
        }
    }
}


#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InferResult {
    pub txid: String,
    pub status: InferStatus,
    pub input: String,
    pub output: String,
    pub output_hash: String,
}


#[allow(unused_variables)]
pub fn infer_chain(txid: String, user_input: &str, context_messages: Option<Vec<ChatCompletionMessage>>) -> Result<InferStatus, Box<dyn error::Error>> {
    let llm_db = db::open(db::LLM_DB_PATH)?;
    let _ = db::sqlite_create(&llm_db, &txid.as_str(), &"", user_input, InferStatus::Created as u8)?;
    Ok(InferStatus::Created)
}


pub fn query(txid: String) -> Result<InferResult, Box<dyn error::Error>> {
    let llm_db = db::open(db::LLM_DB_PATH)?;
    let result = db::sqlite_get(&llm_db, &txid.as_str())?;
    Ok(InferResult{
        txid: result.txid,
        status:  result.status.into(),
        input: result.input,
        output: result.output,
        output_hash: result.output_hash,
    })
}

pub fn query_hash(txid: String) -> Result<InferResult, Box<dyn error::Error>> {
    let llm_db = db::open(db::LLM_DB_PATH)?;
    let result = db::sqlite_get(&llm_db, &txid.as_str())?;
    Ok(InferResult{
        txid: result.txid,
        status: result.status.into(),
        input: "".to_string(),
        output: "".to_string(),
        output_hash: result.output_hash,
    })
}

pub async fn _internal_do_infer() -> Result<(), Box<dyn error::Error>>{
    // todo llm infer
    // 0. open connection
    let llm_db = db::open(db::LLM_DB_PATH)?;
    // 1. get to_do infer row
    let row = db::sqlite_filter_to_infer(&llm_db)?;
    // 2. do infer
    db::sqlite_start_llm(&llm_db, row.txid.as_str(), InferStatus::InProgress as u8)?;
    let result = infer(row.input.as_str(), None).await;
    if !result.is_ok() {
        db::sqlite_end_llm(&llm_db, row.txid.as_str(), "", "", InferStatus::Failure as u8)?;
    } else {
        let output = result.unwrap();
        let output_hash = hex::encode(Sha256Sum::from_data(output.as_bytes()));
        db::sqlite_end_llm(&llm_db, row.txid.as_str(), output.as_str(), output_hash.as_str(), InferStatus::Success as u8)?;
    }
    Ok(())
}

pub fn do_infer() -> Result<(), Box<dyn error::Error>>{
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(_internal_do_infer())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_infer() {
        let user_input = "Is the Earth round?";
        let context_messages = None;

        let result = infer(user_input, context_messages).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        println!("{}", response)
    }

    #[tokio::test]
    async fn test_infer_with_no_userinput() {
        let user_input = "";
        let context_messages = None;

        let result = infer(user_input, context_messages).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_infer_check() {
        let user_input = "Is the Earth round?";
        let context_messages = None;

        let result = infer(user_input, context_messages).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        println!("{}", response);

        let context_messages = None;
        // let response = "不相关的回答";
        let check_result = infer_check(user_input, &response, context_messages).await;
        assert!(check_result.is_ok());

        let res = check_result.unwrap();
        println!("res: {}", res);
    }

    #[tokio::test]
    async fn test_random_question() {

        let result = random_question().await;
        assert!(result.is_ok());

        let response = result.unwrap();
        println!("Question: {}", response);
    }

    #[tokio::test]
    async fn test_infer_chain() {
        let txid = "0".to_string();
        let user_input = "Is the Earth round?";
        let context_messages = None;

        let result = infer_chain(txid, &user_input, context_messages);
        assert!(result.is_ok());
        assert!(result.unwrap() == InferStatus::Created);
    }

    #[tokio::test]
    async fn test_query() {
        let txid = "0".to_string();

        let result = query(txid);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_query_hash() {
        let txid = "0".to_string();

        let result = query_hash(txid);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_query_not_found() {
        let txid = "1".to_string();

        let result = query(txid);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().status, InferStatus::NotFound)
    }

    #[tokio::test]
    async fn test_internal_do_infer() {
        let result = _internal_do_infer().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_do_infer() {
        let result = do_infer();
        assert!(result.is_ok());
    }

    #[test]
    fn test_do_infer_thread() {
        let llm_thread_handle = thread::Builder::new()
            .name("test_thread".to_string())
            .spawn(move || {
                let _ = do_infer();
            })
            .expect("FATAL: failed to spawn chain llm thread");

        llm_thread_handle.join().unwrap();
    }
}