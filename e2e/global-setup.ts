import { execSync } from 'child_process';
import path from 'path';

export default function globalSetup() {
  if (process.env.MBB_BINARY) return;
  const cwd = path.resolve(__dirname, '..');
  console.log('Building Rust binary (release)...');
  execSync('cargo build --release', { stdio: 'inherit', cwd });
}
