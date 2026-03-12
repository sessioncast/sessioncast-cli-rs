# SessionCast CLI

Rust로 작성된 SessionCast CLI - 어디서든 에이전트를 제어하세요.

## 설치

### macOS / Linux

```bash
# Homebrew (추천)
brew install sessioncast/tap/sessioncast

# 또는 직접 다운로드
curl -sL https://github.com/sessioncast/sessioncast-cli/releases/latest/download/sessioncast-$(uname -m)-$(uname -s | tr '[:upper:]' '[:lower:]').tar.gz | tar xz
sudo mv sessioncast /usr/local/bin/
```

### Windows

```powershell
# Scoop
scoop bucket add sessioncast https://github.com/sessioncast/scoop-bucket
scoop install sessioncast

# 또는 직접 다운로드
# https://github.com/sessioncast/sessioncast-cli/releases 에서 sessioncast-x86_64-pc-windows-msvc.zip 다운로드
```

## 사용법

```bash
# 로그인
sessioncast login

# 의존성 확인 및 설치 (tmux/itmux)
sessioncast deps
sessioncast deps install

# 에이전트 시작
sessioncast agent

# tmux 세션에 명령 실행
sessioncast cmd "npm test" -d
sessioncast cmd "make build" -s my-session -d

# 업데이트
sessioncast update
```

## 명령어

| 명령어 | 설명 |
|--------|------|
| `login` | OAuth/PKCE 로그인 |
| `logout` | 로그아웃 |
| `status` | 로그인 상태 확인 |
| `agent` | 에이전트 시작 (tmux 세션 스트리밍) |
| `list` | tmux 세션 목록 |
| `send` | tmux 세션에 키 전송 |
| `cmd` | tmux 세션에서 쉘 명령 실행 |
| `deps` | 의존성 확인/설치 (tmux/itmux) |
| `update` | CLI 업데이트 |
| `tunnel` | 로컬 웹 서비스 터널링 |

## 개발

```bash
# 빌드
cargo build --release

# 테스트
cargo test

# 린트
cargo clippy
cargo fmt --check
```

## 라이선스

MIT
