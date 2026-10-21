#[tokio::main]
async fn main() {
  let mut runtime=deno_core::JsRuntime::new(Default::default());
  let result=runtime.execute_script("sdk-smoke.js", "JSON.stringify({sum:1+2,bytes:new Uint8Array([1,2,3]).length,big:String(10n**2n)})").unwrap();
  println!("V8 {}",deno_core::v8::V8::get_version());
  deno_core::scope!(scope,runtime);
  let result=deno_core::v8::Local::new(scope,result);
  println!("{}",result.to_rust_string_lossy(scope));
}
