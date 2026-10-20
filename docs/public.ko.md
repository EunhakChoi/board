# 팀과 함께 쓰기 (외부 공개 모드)

[English](public.md) | **한국어**

[로컬 모드](local.ko.md) 는 이 컴퓨터 사람이 token 없이 관리자. 외부 공개 모드는 팀 전용 server 용:
주소로 신뢰하는 사람 없음. 팀원은 관리자가 전달한 **token** 으로 입장, 나머지는
방문자 (관리자 설정에 따라 보기 · 댓글 가능 · 입장 불가).

## 준비물

- **고정 주소** 1개 (예: `https://charet.example.com`). 1개면 충분: 미리보기 (프로젝트 코드 실행)
  는 같은 주소의 `/_preview/` 아래에서 sandbox frame (opaque origin) 으로 제공 → 프로젝트 코드가
  도구 화면 · session 에 접근 불가. 미리보기에 별도 origin 을 주려면 `CHARET_PUBLIC_PREVIEW_URL`
  (다른 host 또는 같은 host 의 다른 port). 주소 확보 방법:

  | 방식 | 주소 |
  |---|---|
  | 내 도메인 + reverse proxy 또는 Cloudflare Tunnel | `https://charet.example.com` |
  | Tailscale Funnel (무료, 고정 이름) | `https://pc.tailnet.ts.net` (`tailscale funnel --bg 5280`) |
  | ngrok (무료 고정 domain 1개) | `https://name.ngrok-free.app` (`ngrok http --url=… 5280`) |
  | 사무실 LAN (시험용, 암호화 없음) | `http://192.168.0.10:5280` |

  이름 있는 주소 권장: 전달하는 link 에 IP 주소가 드러나지 않음.
## 시작

`compose.yaml` 옆 `.env` 에 추가:

```bash
CHARET_PUBLIC_URL=https://charet.example.com
# 선택: 미리보기를 별도 origin 으로 (기본은 도구 주소에서 sandbox 로 제공)
# CHARET_PUBLIC_PREVIEW_URL=https://preview.charet.example.com
```

```bash
docker compose -f compose.yaml -f compose.public.yaml up -d
docker compose logs server | grep -A1 "Admin token"
```

Docker 없이: 같은 변수로 `charet-server --public` (`CHARET_PUBLIC_PREVIEW_URL` 대신
`CHARET_PREVIEW_URL`). 고정 주소가 없으면 외부 공개 모드로 시작하지 않음.

점검: 아무 PC 에서 `charet doctor --server https://charet.example.com`. service 외에 HTTPS 주소 여부
확인 (token 이 모든 요청에 실려 감).

## 첫 관리자

server 첫 시작 때 관리자 (이름 *Admin*, 팀 화면에서 변경 가능) 생성 후 log 에 **token** 1회 출력.
`https://charet.example.com/?t=<token>` 을 열거나 화면에서 token 입력. 분실 시
`charet-server admin-token` (Docker: `docker compose exec server charet-server admin-token --public`)
으로 다시 출력.

## 입장

- **팀원** (팀 → 팀원): 이름 · 권한으로 추가 후 token 또는 token 이 담긴 link 전달. 브라우저는
  계속 로그인 유지. 같은 token 으로 다른 브라우저가 들어오면 이전 브라우저 로그아웃 (CLI 는 영향
  없음: `charet login --token …`). **token 재발급** 시 이전 token 중지.
- **방문자**: token 이 없으면 임의 이름으로 입장. 팀 화면의 *방문자* 로 팀 공개 프로젝트에서 할 수 있는
  일 설정: *token 필요* (입장 불가) · *볼 수 있음* (화면 · 흐름 · 코드 · 댓글 읽기) · *댓글 가능*.
  프로젝트별로 **접근** 에서 따로 설정 가능 (*서버 설정* = 팀 화면 설정을 따름) → 한 프로젝트만 외부 댓글을
  받고 나머지는 token 필요로 운영 가능. 비공개 프로젝트는 방문자에게 안 보임, 프로젝트 전체 내려받기
  (`charet checkout`) 는 항상 token 필요.

## 알림 · webhook

- **알림함**: 새 댓글 → 프로젝트를 볼 수 있는 모든 사람. 답글 · 해결 · 다시 열기 → 그 댓글
  thread 참여자. 자기 행동은 알리지 않음. 웹은 상단 bell, agent 는
  `charet inbox`.
- **webhook** (Team → Webhook, 관리자 전용, 로컬 모드에서도 동작): event 선택 (`comment.added`,
  `comment.replied`, `comment.resolved`, `comment.reopened`, `version.created`) + 선택적으로
  프로젝트 1개. 비공개 프로젝트 event 는 그 프로젝트로 지정한
  webhook 에만 전달. 전달 형식 = JSON `POST`:

  ```json
  { "event": "comment.added", "project": "web", "actor": "Ann",
    "text": "Ann commented on screen home: …", "url": "https://charet.example.com/p/web/…",
    "data": { "comment": 12, "kind": "screen", "target": "home", "body": "…" } }
  ```

  검증: `X-Charet-Signature: sha256=<hex>` = webhook 생성 때 표시된 secret 으로 계산한
  `<X-Charet-Timestamp>.<body>` 의 HMAC-SHA256. 실패 시 2번 재시도 (2초 · 10초 후). Team 화면에
  마지막 상태 표시. "테스트 전송" = `ping` 전송.

## 보안

- 브라우저 session = `HttpOnly` · `SameSite=Lax` cookie, HTTPS 에서 `Secure`, 1년. 변경 요청은
  `x-charet-client` header 도 필요 (다른 site 는 보낼 수 없음).
- 팀원 token 은 관리자가 다시 전달할 수 있도록 그대로 저장, session cookie 는 hash 만 저장. 팀원
  삭제 · token 재발급 시 그 사람의 session 즉시 종료.
- webhook secret 은 server key (`CHARET_SECRET_KEY` 또는 `data/secret.key`) 에서 파생, 저장 안 함.
  key 를 바꾸면 모든 webhook secret 도 바뀜.
