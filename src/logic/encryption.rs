use crate::{APPNAME, logic::global::FileDir};
use argon2::password_hash::PasswordHasher;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use std::{io::{BufRead, Read, Write}, u16};

static SIZE_1MB: usize = 1024 * 1024;

static FILE_HEADER_SIZE: usize = SIZE_1MB; // 1 MB

static FILE_1GB: usize = 1024 * 1024 * 1024; // 1 GB
static CHUNK: usize = SIZE_1MB; // 1 MB
// static CHUNK_4KB: usize = 0x1000; // 4 KB

static NONCE_SIZE: usize = 24;
static ENCRYPTION_EXT: &str = "snc";
static BENCHMARK_EXT: &str = "benchmark";
static ENCRYPTION_VERSION: u16 = 1;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EncMethod {
  #[default]
  XChaCha20 = 1,
}

impl EncMethod {
  pub fn from_u16(num: u16) -> Option<Self> {
    match num {
      1 => Some(EncMethod::XChaCha20),
      _ => None,
    }
  }
}

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
pub struct FileHeader {
  pub packed: bool,
  pub file: bool,
  pub version: u16,
  pub encryption: EncMethod,
  pub name_len: u16,
  pub name: String,
  pub path_len: u16,
  pub path: std::path::PathBuf,
}

impl FileHeader {
  pub fn new(packed: bool, file: bool, version: u16, encryption: EncMethod, name: impl AsRef<str>, path: impl AsRef<std::path::Path>) -> Self {
    let name = name.as_ref().to_string();
    let path = path.as_ref().to_path_buf();
    Self {
      packed,
      file,
      version,
      encryption,
      name_len: name.len() as u16,
      name,
      path_len: path.to_str().unwrap().len() as u16,
      path,
    }
  }

  pub fn to_vec(&self) -> Vec<u8> {
    let packed = self.packed as u16;
    let file = self.file as u16;
    let version = self.version;
    let encryption = self.encryption as u16;
    let name_len = self.name.len() as u16;
    let name_bytes = self.name.as_bytes();
    let path_len = self.path_len;
    let path = self.path.to_str().unwrap().as_bytes();

    let mut file_header = vec![0u8; FILE_HEADER_SIZE];
    let mut pos = 0; // Track current write position

    let var_size = size_of::<u16>();

    // Write packed (2 bytes)
    file_header[pos..pos + var_size].copy_from_slice(&packed.to_le_bytes());
    pos += var_size;

    // Write file (2 bytes)
    file_header[pos..pos + var_size].copy_from_slice(&file.to_le_bytes());
    pos += var_size;

    // Write version (2 bytes)
    file_header[pos..pos + var_size].copy_from_slice(&version.to_le_bytes());
    pos += var_size;

    // Write encryption (2 bytes)
    file_header[pos..pos + var_size].copy_from_slice(&encryption.to_le_bytes());
    pos += var_size;

    // Write name_len (2 bytes)
    file_header[pos..pos + var_size].copy_from_slice(&name_len.to_le_bytes());
    pos += var_size;

    // Write name_bytes (variable length)
    let name_end = pos + name_bytes.len();
    file_header[pos..name_end].copy_from_slice(name_bytes);
    pos += name_bytes.len();

    // Write path_len (2 bytes)
    file_header[pos..pos + var_size].copy_from_slice(&path_len.to_le_bytes());
    pos += var_size;

    // Write path (2 bytes)
    let path_end = pos + path.len();
    file_header[pos..path_end].copy_from_slice(path);
    pos += path.len();

    file_header
  }

  pub fn from_vec(vec: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
    if vec.len() != FILE_HEADER_SIZE {
      return Err("Incorrect header".into());
    }

    let mut pos = 0; // Track current write position
    let var_size = size_of::<u16>();

    let packed_slice = if let Some(slice) = vec.get(pos..pos + var_size) { slice } else { return Err("Failed to get slice".into()) };

    let packed = u16::from_le_bytes(packed_slice.try_into()?) != 0;
    pos += var_size;

    let file_slice = if let Some(slice) = vec.get(pos..pos + var_size) { slice } else { return Err("Failed to get slice".into()) };

    let file = u16::from_le_bytes(file_slice.try_into()?) != 0;
    pos += var_size;

    let version_slice = if let Some(slice) = vec.get(pos..pos + var_size) { slice } else { return Err("Failed to get slice".into()) };

    let version = u16::from_le_bytes(version_slice.try_into()?);
    pos += var_size;

    let encryption_num_slice = if let Some(slice) = vec.get(pos..pos + var_size) { slice } else { return Err("Failed to get slice".into()) };

    let encryption_num = u16::from_le_bytes(encryption_num_slice.try_into()?);
    pos += var_size;

    let name_len_slice = if let Some(slice) = vec.get(pos..pos + var_size) { slice } else { return Err("Failed to get slice".into()) };

    let name_len = u16::from_le_bytes(name_len_slice.try_into()?) as usize;
    pos += var_size;

    let name_slice = if let Some(slice) = vec.get(pos..pos + name_len) { slice } else { return Err("Failed to get slice".into()) };

    let name = match String::from_utf8(name_slice.to_vec()) {
      Ok(v) => v,
      Err(e) => return Err(format!("Invalid UTF-8 in name {}", e).into()),
    };
    pos += name_len;

    let path_slice = if let Some(slice) = vec.get(pos..pos + var_size) { slice } else { return Err("Failed to get slice".into()) };

    let path_len = u16::from_le_bytes(path_slice.try_into()?) as usize;
    pos += var_size;

    let path_slice = if let Some(slice) = vec.get(pos..pos + path_len) { slice } else { return Err("Failed to get slice".into()) };

    let path_str = match String::from_utf8(path_slice.to_vec()).map_err(|_| "Invalid UTF-8 in name") {
      Ok(v) => v,
      Err(e) => return Err(format!("Invalid UTF-8 in name {}", e).into()),
    };

    let path = std::path::PathBuf::from(path_str);
    pos += path_len;

    let encryption = EncMethod::from_u16(encryption_num).ok_or("Invalid encryption method")?;

    Ok(Self {
      packed,
      file,
      version,
      encryption,
      name_len: name_len as u16,
      name,
      path_len: path_len as u16,
      path,
    })
  }
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
    // Validate input path exists
    if !self.input_path.exists() {
      return Err(format!("Input path does not exist: {:?}", self.input_path));
    }

    let file = FileDir::what(&self.input_path).map(|v| if v == FileDir::Directory { true } else { false }).unwrap();
    let packed = file;

    // Get file name with better error handling
    let file_name = self.input_path.file_name().ok_or_else(|| "Input path has no file name".to_string())?.to_str().ok_or_else(|| "File name is not valid UTF-8".to_string())?;

    // Create file header
    let file_h = FileHeader::new(packed, file, ENCRYPTION_VERSION, EncMethod::XChaCha20, file_name, &self.input_path);
    let mut file_h_vec = file_h.to_vec();

    // Get file size with error handling
    let file_size = fs_extra::dir::get_size(self.input_path.clone()).map_err(|e| format!("Failed to get input size: {}", e))? as usize;

    // Prepare encryption
    let salt = Self::get_salt(None);
    let key = Self::get_key(self.password.clone(), &salt);
    let nonce = Self::gen_nonce();
    let mut cipher = chacha20::XChaCha20::new(key.as_bytes().into(), &nonce.into());

    let def_output = self.output_dir.join(file_name).with_extension(ENCRYPTION_EXT);

    // Create output file with better error handling
    let file_path = if self.input_path == def_output {
      let mut v = def_output;
      v.set_file_name(format!("{} - new", self.input_path.file_stem().unwrap().to_str().unwrap()));
      v.set_extension(ENCRYPTION_EXT);
      v
    } else {
      def_output
    };

    let mut out_file = match std::fs::File::create(&file_path) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to create output file at {:?}: {}", file_path, e)),
    };

    // Write salt + nonce with error handling
    match writeln!(out_file, "{}", salt.as_str()) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to write salt: {}", e)),
    };
    match out_file.write_all(&nonce) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to write nonce: {}", e)),
    };

    // Write encrypted file info
    cipher.apply_keystream(&mut file_h_vec);
    match out_file.write_all(&file_h_vec) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to write file header: {}", e)),
    };

    // Create an encrypting writer that wraps the output file
    let mut encrypting_writer = EncryptingWriter::new(out_file, cipher);

    // Set progress sender if provided
    if let Some(sender) = self.progress.clone() {
      encrypting_writer.set_progress_sender(sender);
    }

    encrypting_writer.set_total_input_size(file_size);

    if packed {
      // Stream the tar with better error handling
      let mut tar_builder = tar::Builder::new(&mut encrypting_writer);

      let result = if self.input_path.is_dir() { tar_builder.append_dir_all(file_name, &self.input_path) } else { tar_builder.append_path_with_name(&self.input_path, file_name) };

      match result {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to add files to archive: {}", e)),
      };

      match tar_builder.finish() {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to finalize archive: {}", e)),
      };
    } else {
      // Open input file for reading
      let mut in_file = match std::fs::File::open(&self.input_path) {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to open input file: {}", e)),
      };

      // Stream the input file through the encrypting writer
      match std::io::copy(&mut in_file, &mut encrypting_writer) {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to write encrypted file: {}", e)),
      };
    }

    match encrypting_writer.flush() {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to flush writer: {}", e)),
    };

    Ok(())
  }

  pub fn decrypt_file(&self) -> Result<(), String> {
    // Open input file
    let mut in_file = match std::fs::File::open(&self.input_path) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to open input file: {}", e)),
    };
    let mut buf_reader = std::io::BufReader::new(&mut in_file);

    // File size for progress
    let file_size = std::fs::metadata(&self.input_path).map_err(|e| format!("Failed to get file size: {}", e))?.len() as usize;

    // 1. Read salt (text line, not encrypted)
    let mut salt_str = String::new();
    match buf_reader.read_line(&mut salt_str) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to read salt: {}", e)),
    };
    let salt = match argon2::password_hash::SaltString::from_b64(salt_str.trim()) {
      Ok(v) => v,
      Err(e) => return Err(format!("Invalid salt format: {}", e)),
    };

    // 2. Read nonce (not encrypted)
    let mut nonce = [0u8; 24];
    match buf_reader.read_exact(&mut nonce) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to read nonce: {}", e)),
    };

    // 3. Prepare cipher
    let key = Self::get_key(self.password.clone(), &salt);
    let cipher = chacha20::XChaCha20::new(key.as_bytes().into(), &nonce.into());

    // 4. Wrap the reader so ALL encrypted bytes come through the decryptor
    let mut decrypting_reader = DecryptingReader::new(buf_reader, cipher);

    // 5. Read and parse header directly from decrypting reader
    let mut header = [0u8; FILE_HEADER_SIZE];
    match decrypting_reader.read_exact(&mut header) {
      Ok(v) => v,
      Err(e) => return Err(format!("Failed to read file header: {}", e)),
    };
    let file_h = match FileHeader::from_vec(&header.to_vec()) {
      Ok(v) => v,
      Err(e) => return Err(format!("Invalid file header: {}", e)),
    };

    // 6. Progress tracking (still using the same decrypting_reader)
    if let Some(sender) = self.progress.clone() {
      decrypting_reader.set_progress_sender(sender);
      decrypting_reader.set_total_input_size(file_size);
    }

    if file_h.packed {
      // 7. Extract tar archive (already positioned after header)
      let mut tar_archive = tar::Archive::new(decrypting_reader);
      match tar_archive.unpack(&self.output_dir) {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to unpack archive: {}", e)),
      };
    } else {
      // 7. Output the single file (already positioned after header)
      let output_path = self.output_dir.join(&file_h.name);
      let mut out_file = match std::fs::File::create(&output_path) {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to create output file at {:?}: {}", output_path, e)),
      };

      // Copy all remaining decrypted data into the output file
      match std::io::copy(&mut decrypting_reader, &mut out_file) {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to write decrypted file: {}", e)),
      };
    }

    Ok(())
  }

  pub fn benchmark() -> Result<(std::time::Duration, std::time::Duration), String> {
    std::thread::Builder::new()
      .stack_size(SIZE_1MB * 4)
      .spawn(|| {
        let path = match Self::gen_file() {
          Ok(v) => v,
          Err(e) => return Err(e),
        };

        let output_dir = path.parent().unwrap();

        let encrypt_time = {
          let time = std::time::Instant::now();

          let shincrypt = ShinCrypt::new(&path, output_dir, APPNAME, None);
          if let Err(e) = shincrypt.encrypt_file() {
            println!("{}", e);
          };

          time.elapsed()
        };

        let mut input_path = path.clone();
        input_path.set_extension(ENCRYPTION_EXT);

        let decrypt_time = {
          let time = std::time::Instant::now();

          let shincrypt = ShinCrypt::new(input_path, output_dir, APPNAME, None);
          if let Err(e) = shincrypt.decrypt_file() {
            println!("{}", e);
          };

          time.elapsed()
        };

        std::fs::remove_dir_all(output_dir).unwrap();
        Ok((encrypt_time, decrypt_time))
      })
      .unwrap()
      .join()
      .unwrap()
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
