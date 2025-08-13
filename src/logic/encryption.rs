use crate::APPNAME;
use argon2::password_hash::PasswordHasher;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use std::io::{BufRead, Read, Write};

static FILE_1GB: usize = 1024 * 1024 * 1024; // 1 GB
static CHUNK_1MB: usize = 1024 * 1024; // 1 MB
// static CHUNK_4KB: usize = 0x1000; // 4 KB

static CHUNK: usize = CHUNK_1MB;

static NONCE_SIZE: usize = 24;
static ENCRYPTION_EXT: &str = APPNAME;
static BENCHMARK_EXT: &str = "benchmark";

struct EncryptingWriter<W: Write> {
  inner: W,
  cipher: chacha20::XChaCha20,
  buffer: Vec<u8>,
  progress_sender: Option<crossbeam::channel::Sender<f64>>, // Sends progress as a fraction (0.0 to 1.0)
  total_bytes_processed: usize,
  total_input_size: Option<usize>, // Optional: Needed for percentage calculation
}

impl<W: Write> EncryptingWriter<W> {
  fn new(inner: W, cipher: chacha20::XChaCha20) -> Self {
    Self {
      inner,
      cipher,
      buffer: Vec::with_capacity(CHUNK),
      progress_sender: None,
      total_bytes_processed: 0,
      total_input_size: None,
    }
  }

  // Set the progress sender (if you want to track progress)
  fn set_progress_sender(&mut self, sender: crossbeam::channel::Sender<f64>) { self.progress_sender = Some(sender); }

  // Set total input size (if known, for percentage tracking)
  fn set_total_input_size(&mut self, size: usize) { self.total_input_size = Some(size); }

  fn send_progress_update(&self) {
    if let Some(sender) = &self.progress_sender {
      let progress = if let Some(total_size) = self.total_input_size {
        // Send as a fraction (0.0 to 1.0)
        self.total_bytes_processed as f64 / total_size as f64
      } else {
        // Just send raw bytes if total size is unknown
        self.total_bytes_processed as f64
      };
      sender.send(progress).unwrap(); // Ignore errors if receiver is dropped
    }
  }
}

impl<W: Write> Write for EncryptingWriter<W> {
  fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    self.buffer.extend_from_slice(buf);

    while self.buffer.len() >= CHUNK {
      let chunk = &mut self.buffer[..CHUNK];

      self.cipher.apply_keystream(chunk);

      self.inner.write_all(chunk)?;

      self.buffer.drain(..CHUNK);

      // Update progress
      self.total_bytes_processed += CHUNK;
      self.send_progress_update();
    }

    Ok(buf.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    // Process remaining bytes
    if !self.buffer.is_empty() {
      let remaining = self.buffer.len();
      self.cipher.apply_keystream(&mut self.buffer);
      self.inner.write_all(&self.buffer).unwrap();
      self.buffer.clear();

      // Update progress for remaining bytes
      self.total_bytes_processed += remaining;
      self.send_progress_update();
    }
    self.inner.flush()
  }
}

struct DecryptingReader<R: Read> {
  inner: R,
  cipher: chacha20::XChaCha20,
  buffer: Vec<u8>,
  pos: usize,
  progress_sender: Option<crossbeam::channel::Sender<f64>>, // Sends progress as a fraction (0.0 to 1.0)
  total_bytes_processed: usize,
  total_input_size: Option<usize>, // Optional: Needed for percentage calculation
}

impl<R: Read> DecryptingReader<R> {
  fn new(inner: R, cipher: chacha20::XChaCha20) -> Self {
    Self {
      inner,
      cipher,
      buffer: Vec::new(),
      pos: 0,
      progress_sender: None,
      total_bytes_processed: 0,
      total_input_size: None,
    }
  }

  // Set the progress sender (if you want to track progress)
  fn set_progress_sender(&mut self, sender: crossbeam::channel::Sender<f64>) { self.progress_sender = Some(sender); }

  // Set total input size (if known, for percentage tracking)
  fn set_total_input_size(&mut self, size: usize) { self.total_input_size = Some(size); }

  fn send_progress_update(&self) {
    if let Some(sender) = &self.progress_sender {
      let progress = if let Some(total_size) = self.total_input_size {
        // Send as a fraction (0.0 to 1.0)
        self.total_bytes_processed as f64 / total_size as f64
      } else {
        // Just send raw bytes if total size is unknown
        self.total_bytes_processed as f64
      };
      sender.send(progress).unwrap(); // Ignore errors if receiver is dropped
    }
  }
}

impl<R: Read> Read for DecryptingReader<R> {
  fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
    if self.pos == self.buffer.len() {
      // refill buffer
      self.buffer.resize(CHUNK, 0);
      let n = self.inner.read(&mut self.buffer).unwrap();
      self.buffer.truncate(n);
      if n == 0 {
        return Ok(0);
      }

      // decrypt the buffer in place
      self.cipher.apply_keystream(&mut self.buffer);
      self.pos = 0;

      // Update progress after decrypting each chunk
      self.total_bytes_processed += n;
      self.send_progress_update();
    }

    let n = std::cmp::min(buf.len(), self.buffer.len() - self.pos);
    buf[..n].copy_from_slice(&self.buffer[self.pos..self.pos + n]);
    self.pos += n;

    Ok(n)
  }
}

#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct FileInfo {
  pub is_file: bool,
  pub name_len: u8,
  pub name: String,
  pub path: std::path::PathBuf,
}

impl FileInfo {
  pub fn new(is_file: bool, name: impl AsRef<str>, path: std::path::PathBuf) -> Self {
    let name = name.as_ref().to_string();
    Self { is_file, name_len: name.len() as u8, name, path }
  }

  pub fn to_vec(&self) -> Vec<u8> {
    let is_file = [self.is_file as u8];
    let name_len = [self.name.len() as u8];
    let name_vec = self.name.as_bytes().to_vec();

    let mut file_info = Vec::with_capacity(2 + std::u8::MAX as usize);
    file_info.extend_from_slice(&is_file);
    file_info.extend_from_slice(&name_len);
    file_info.extend_from_slice(&name_vec);

    file_info
  }

  pub fn from_vec(vec: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
    let is_file = vec[0] != 0;
    let name_len = vec[1];
    let struct_len = 2 + name_len as usize;
    let name = String::from_utf8(vec[2..struct_len].to_vec()).unwrap();

    Ok(Self { is_file, name_len, name, path: std::path::PathBuf::default() })
  }

  // pub fn len(&self) -> usize { self.name.len() + 2 }
}

pub struct ShinCrypt {
  input_path: std::path::PathBuf,
  output_dir: std::path::PathBuf,
  password: String,
  progress: Option<crossbeam::channel::Sender<f64>>,
}

impl ShinCrypt {
  pub fn new(input_path: impl AsRef<std::path::Path>, output_dir: impl AsRef<std::path::Path>, password: impl AsRef<str>, progress: Option<crossbeam::channel::Sender<f64>>) -> Self {
    return Self {
      input_path: input_path.as_ref().to_path_buf(),
      output_dir: output_dir.as_ref().to_path_buf(),
      password: password.as_ref().into(),
      progress,
    };
  }

  fn get_salt(salt: Option<String>) -> argon2::password_hash::SaltString { if salt.is_some() { argon2::password_hash::SaltString::from_b64(salt.unwrap().trim()).unwrap() } else { argon2::password_hash::SaltString::generate(&mut argon2::password_hash::rand_core::OsRng) } }

  fn get_key(password: String, salt: &argon2::password_hash::SaltString) -> argon2::password_hash::Output {
    let argon2 = argon2::Argon2::default();
    let password_hash = argon2.hash_password(password.as_bytes(), salt).unwrap();
    let key = password_hash.hash.unwrap();

    key
  }

  fn gen_nonce() -> [u8; NONCE_SIZE] {
    let mut nonce = [0u8; NONCE_SIZE];
    let mut rng = argon2::password_hash::rand_core::OsRng;
    argon2::password_hash::rand_core::RngCore::fill_bytes(&mut rng, &mut nonce);

    nonce
  }

  // pub fn encrypt_chunk(cipher: &mut chacha20::cipher::StreamCipherCoreWrapper<chacha20::XChaChaCore<chacha20::cipher::typenum::UInt<chacha20::cipher::typenum::UInt<chacha20::cipher::typenum::UInt<chacha20::cipher::typenum::UInt<chacha20::cipher::typenum::UTerm, chacha20::cipher::consts::B1>, chacha20::cipher::consts::B0>, chacha20::cipher::consts::B1>, chacha20::cipher::consts::B0>>>, chunk: &mut [u8]) { cipher.apply_keystream(chunk); }

  pub fn encrypt_file(&self) -> Result<(), String> {
    // let time = std::time::Instant::now();

    let file_info = FileInfo::new(self.input_path.is_file(), self.input_path.file_name().unwrap().to_str().unwrap(), self.input_path.clone());
    let mut file_info_vec = file_info.to_vec();

    let file_size = fs_extra::dir::get_size(self.input_path.clone()).unwrap() as usize;

    // Prepare encryption
    let salt = Self::get_salt(None);
    let key = Self::get_key(self.password.clone(), &salt);
    let nonce = Self::gen_nonce();
    let mut cipher = chacha20::XChaCha20::new(key.as_bytes().into(), &nonce.into());

    // Create output file
    let file_name = self.input_path.file_name().unwrap();
    let file_path = self.output_dir.join(file_name).with_extension(ENCRYPTION_EXT);
    let mut out_file = std::fs::File::create(file_path).map_err(|e| format!("Invalid path\n{}", e))?;

    // Write salt + nonce
    writeln!(out_file, "{}", salt.as_str()).unwrap();
    out_file.write_all(&nonce).unwrap();

    // Write encrypted file info
    cipher.apply_keystream(&mut file_info_vec);
    out_file.write_all(&file_info_vec).unwrap();

    // Create an encrypting writer that wraps the output file
    let mut encrypting_writer = EncryptingWriter::new(out_file, cipher);

    // Set progress sender if provided
    if let Some(sender) = self.progress.clone() {
      encrypting_writer.set_progress_sender(sender);
    }

    encrypting_writer.set_total_input_size(file_size);

    // // Optionally, set total input size (if you can compute it)
    // if let Ok(metadata) = std::fs::metadata(&self.input_path) {
    //   encrypting_writer.set_total_input_size(metadata.len() as usize);
    // }

    // Stream the tar directly to the encrypting writer
    {
      let mut tar_builder = tar::Builder::new(&mut encrypting_writer);
      if self.input_path.is_dir() {
        tar_builder.append_dir_all(self.input_path.file_name().unwrap(), &self.input_path).unwrap();
      } else {
        tar_builder.append_path_with_name(&self.input_path, self.input_path.file_name().unwrap()).unwrap();
      }
      tar_builder.finish().unwrap();
    }

    encrypting_writer.flush().unwrap();

    // dbg!(time.elapsed());

    Ok(())
  }

  pub fn decrypt_file(&self) -> Result<FileInfo, String> {
    // let time = std::time::Instant::now();

    let mut in_file = match std::fs::File::open(&self.input_path).map_err(|e| e.to_string()) {
      Ok(v) => v,
      Err(_) => return Err(format!("Failed to decrypt file")),
    };
    let mut buf_reader = std::io::BufReader::new(&mut in_file);

    let file_size = fs_extra::dir::get_size(self.input_path.clone()).unwrap() as usize;

    // 1. Read salt line (text)
    let mut salt_str = String::new();
    buf_reader.read_line(&mut salt_str).map_err(|e| e.to_string()).unwrap();
    let salt = match argon2::password_hash::SaltString::from_b64(salt_str.trim()) {
      Ok(v) => v,
      Err(_) => return Err(format!("Can't decrypt file")),
    };

    // 2. Read nonce bytes
    let mut nonce = [0u8; 24];
    buf_reader.read_exact(&mut nonce).map_err(|e| e.to_string()).unwrap();

    // 3. Prepare cipher for decryption
    let key = Self::get_key(self.password.clone(), &salt);
    let mut cipher = chacha20::XChaCha20::new(key.as_bytes().into(), &nonce.into());

    // 4. Read and decrypt first 2 bytes to get the length
    let mut first_two = [0u8; 2];
    buf_reader.read_exact(&mut first_two).map_err(|e| e.to_string()).unwrap();
    cipher.apply_keystream(&mut first_two);

    // The length is in the second byte
    let length = first_two[1] as usize;

    // 5. Read the rest of the FileInfo bytes according to length
    let mut rest = vec![0u8; length];
    buf_reader.read_exact(&mut rest).map_err(|e| e.to_string()).unwrap();
    cipher.apply_keystream(&mut rest);

    // 6. Combine the two parts
    let mut file_info_vec = Vec::with_capacity(2 + length);
    file_info_vec.extend_from_slice(&first_two);
    file_info_vec.extend_from_slice(&rest);

    // 7. Parse FileInfo from decrypted bytes
    let file_info = FileInfo::from_vec(&file_info_vec).unwrap();

    // 8. Wrap the rest of the file in DecryptingReader
    let mut decrypting_reader = DecryptingReader::new(buf_reader, cipher);

    // Set up progress tracking
    if let Some(sender) = self.progress.clone() {
      decrypting_reader.set_progress_sender(sender);
      decrypting_reader.set_total_input_size(file_size);
    }

    // 9. Extract tar archive from decrypted stream
    let mut tar_archive = tar::Archive::new(decrypting_reader);
    tar_archive.unpack(&self.output_dir).map_err(|e| e.to_string()).unwrap();

    // dbg!(time.elapsed());

    Ok(file_info)
  }

  pub fn benchmark() -> Result<(std::time::Duration, std::time::Duration), String> {
    let path = match Self::gen_file() {
      Ok(v) => v,
      Err(e) => return Err(e),
    };

    let output_dir = path.parent().unwrap();

    let encrypt_time = {
      let time = std::time::Instant::now();

      let shincrypt = ShinCrypt::new(&path, output_dir, APPNAME, None);
      shincrypt.encrypt_file().unwrap();

      time.elapsed()
    };

    let mut input_path = path.clone();
    input_path.set_extension(ENCRYPTION_EXT);

    let decrypt_time = {
      let time = std::time::Instant::now();

      let shincrypt = ShinCrypt::new(input_path, output_dir, APPNAME, None);
      shincrypt.decrypt_file().unwrap();

      time.elapsed()
    };

    std::fs::remove_dir_all(output_dir).unwrap();

    Ok((encrypt_time, decrypt_time))
  }

  fn gen_file() -> Result<std::path::PathBuf, String> {
    let current_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let benchmark_dir = current_dir.join(BENCHMARK_EXT);
    std::fs::create_dir_all(&benchmark_dir).unwrap();
    let file_path = benchmark_dir.join(ENCRYPTION_EXT).with_extension(BENCHMARK_EXT);

    let file = std::fs::File::create(file_path.clone()).unwrap();
    let mut file_buf = std::io::BufWriter::new(file);

    let mut buffer = vec![0u8; FILE_1GB];

    getrandom::fill(&mut buffer).unwrap();
    file_buf.write_all(&buffer).unwrap();
    Ok(file_path)
  }
}
