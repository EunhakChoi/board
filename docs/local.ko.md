# board 로컬 실행

[English](local.md) | **한국어**

board 는 service 3개로 동작함: **board-server** (API · 웹 도구 · 미리보기), **board-workspace**
(프로젝트 코드 install · build), **board-render** (Chromium: 캡처 · 검사). 로컬 모드는 로그인 없음 —
port 에 닿는 사람은 모두 내장 관리자로 동작하므로 port 는 이 컴퓨터의 loopback 에만 열림.

## Docker 로 실행 (권장)

Docker · Compose v2 필요.

```bash
git clone https://github.com/EunhakChoi/board.git
cd board
docker compose up -d
```

**http://localhost:5280** 접속 — `127.0.0.1` 아닌 `localhost`. 미리보기는 다른 site 인
`127.0.0.1:5281` 에서 제공 → 프로젝트 코드가 도구의 cookie 를 읽을 수 없음.

첫 image build 는 시간이 걸림 (Rust · 웹 도구 compile).

| 설정 (환경 변수 또는 `.env`) | 기본값 | |
|---|---|---|
| `BOARD_PORT` | `5280` | 웹 도구 · API |
| `BOARD_PREVIEW_PORT` | `5281` | 미리보기 |
| `BOARD_UPDATE_CHECK` | `true` | 하루 1회 새 버전 확인 (유일한 외부 호출) |

데이터는 Docker volume 에 보관:

| Volume | 내용 |
|---|---|
| `board_data` | 프로젝트 (git) · 버전 · 댓글 · 캡처 · 미리보기 서명 key |
| `board_workspace` | 의존성 cache · build 결과 (지워도 됨, 필요할 때 다시 build) |
| `board_secrets` | service 끼리 쓰는 token |

```bash
docker compose logs -f server        # log
docker compose up -d --build         # `git pull` 후 갱신
docker compose down                  # 중지 (데이터 유지)
docker compose down -v               # 중지 + 데이터 전부 삭제

# 프로젝트 · 댓글 · 캡처 백업
docker run --rm -v board_data:/data -v "$PWD":/backup debian:trixie-slim \
  tar -czf /backup/board-data.tgz -C /data .
```

## CLI

CLI 는 아직 배포 전 — clone 에서 실행 (Node.js 24+, pnpm 12):

```bash
pnpm install
alias board="node $PWD/apps/board-cli/src/main.ts"
board doctor                          # node · git · server · 내부 service 점검
board setup                           # 첫 설치 안내, Claude Code / Codex 용 skill 설치 제안
board init demo                       # 권장 template 으로 새 프로젝트
board import ../my-app                # 또는 기존 앱 (쓰는 workspace package 포함)
```

`BOARD_URL` 로 다른 server 지정 (기본 `http://127.0.0.1:5280`).

## Docker 없이 실행

Rust (stable) · Node.js 24 · pnpm 12 · git · Chromium 필요
(또는 `pnpm --filter board-web exec playwright install chromium-headless-shell`).

```bash
make setup
cargo build --release -p board-server -p board-render
pnpm --filter board-web run build
node apps/board-cli/src/main.ts serve    # server + workspace + render, Ctrl-C 로 모두 중지
```

데이터는 `~/.board` (`BOARD_HOME`) 에 저장.

## 보안 참고

- 로컬 모드는 port 에 닿는 모든 사람을 신뢰함. loopback 밖으로 port 공개 금지 — 로그인이 있는 공개
  모드는 이후 단계.
- 프로젝트 코드 (의존성 install · build) 는 board-workspace 안에서만 실행, install script 끔 · 최소
  환경 변수. 미리보기는 별도 origin 의 browser 에서 실행, 서명된 짧은 수명 token 사용.
- 외부 호출은 하루 1회 새 버전 확인뿐 (`BOARD_UPDATE_CHECK=false` 로 끔). 코드 · 화면 · 사용 정보
  전송 없음.

## 문제 해결

- `board doctor` 가 무엇이 문제인지와 시작 방법을 알려 줌.
- 도구에서 미리보기가 비어 보임: 도구를 `127.0.0.1` 아닌 `http://localhost:<port>` 로 열기.
- port 충돌: `BOARD_PORT` / `BOARD_PREVIEW_PORT` 지정 후 `docker compose up -d` 다시 실행.
