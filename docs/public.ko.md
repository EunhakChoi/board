# 팀과 함께 쓰기 (외부 공개 모드)

[English](public.md) | **한국어**

[로컬 모드](local.ko.md) 는 port 에 닿는 모든 사람을 관리자로 취급함. 외부 공개 모드는 팀용:
모두 로그인하고, 누가 들어올지는 관리자가 정함.

## 준비물

- **고정 주소** 2개 (도구용 · 미리보기용, 모두 HTTPS). 예: `https://board.example.com`,
  `https://preview.board.example.com`. port 5280 · 5281 앞에 reverse proxy (Caddy, nginx) 또는
  고정 domain tunnel (Cloudflare Tunnel, ngrok 예약 domain). 미리보기는 프로젝트 코드를
  실행하므로 도구와 다른 host 여야 함 (도구 cookie 접근 차단).
- **OpenID Connect client.** 기본은 Google:
  1. Google Cloud console → *APIs & Services* → *Credentials* → *Create credentials* →
     *OAuth client ID* → *Web application*.
  2. Authorized redirect URI: `https://board.example.com/api/auth/oidc/callback`.
  3. client ID · secret 보관.

  다른 OIDC provider 도 가능: `BOARD_OIDC_ISSUER` 에 issuer URL 지정.

## 시작

`compose.yaml` 옆 `.env` 에 추가:

```bash
BOARD_PUBLIC_URL=https://board.example.com
BOARD_PUBLIC_PREVIEW_URL=https://preview.board.example.com
BOARD_OIDC_CLIENT_ID=…
BOARD_OIDC_CLIENT_SECRET=…
# 선택: 첫 로그인 때 관리자가 되는 주소
BOARD_ADMIN_EMAILS=you@example.com
```

```bash
docker compose -f compose.yaml -f compose.public.yaml up -d
docker compose logs server | grep -A1 "setup link"
```

Docker 없이: 같은 변수로 `board-server --public` (`BOARD_PUBLIC_PREVIEW_URL` 대신
`BOARD_PREVIEW_URL`). 로그인 설정이나 고정 주소가 없으면 외부 공개 모드로 시작하지 않음.

## 첫 관리자

관리자가 생기기 전에는 아무도 못 들어옴. server 가 시작할 때 log 에 **1회용 설정 link** 를
출력함. 이 link 로 처음 로그인한 계정이 관리자. `BOARD_ADMIN_EMAILS` 의 주소는 바로 관리자가 됨.
link 는 한 번 쓰면 끝. 관리자가 없는 동안은 시작할 때마다 새 link 출력.

## 입장

- **허용 목록** 의 주소 · domain: 첫 로그인 때 바로 입장 (editor).
- 그 밖의 사람: 관리자가 **접속 승인 요청** 을 승인 · 거절할 때까지 "승인 대기 중" 화면.

## 보안

- session = `HttpOnly` · `SameSite=Lax` cookie, HTTPS 에서 `Secure`, 30일. 변경 요청은
  `x-board-client` header 도 필요 (다른 site 는 보낼 수 없음).
- token 은 hash 만 저장. 거절 · 권한 회수 시 그 사람의 session 즉시 종료.
- client secret 은 파일로도 지정 가능: `BOARD_OIDC_CLIENT_SECRET_FILE`.
