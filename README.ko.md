# charet

*React 와 AI agent 를 위한 live-code design charrette.*

[English](README.md) | **한국어**

팀을 위한 코드 기반 디자인 보드. 실제 UI 코드(React + TypeScript)를 mock API 로 render 해 component · page · 화면으로 펼쳐 보여 줌.
팀원은 댓글을 달거나 AI agent · 손 편집으로 수정안을 내고, 채택하면 버전으로 남음 — 디자인과 코드가 어긋나지 않음.

> 상태: pre-alpha, 비공개. 아직 사용 불가.

## 실행

```bash
git clone https://github.com/EunhakChoi/charet.git && cd charet
./start.sh     # Docker 로 시작, 관리자 link 출력 (처음엔 브라우저로 열기까지)
```

설정 · CLI · Docker 없이 실행 · 보안 참고: [docs/local.ko.md](docs/local.ko.md).
변경이 임시 수정안에서 버전이 되기까지: [docs/workflow.ko.md](docs/workflow.ko.md).
Web tool 에서 coding agent 에게 변경 요청: [docs/agent.ko.md](docs/agent.ko.md).
텍스트 · variant · 간격 · 순서 직접 수정: [docs/hand-edits.ko.md](docs/hand-edits.ko.md).

## License

[MIT](LICENSE-MIT) 또는 [Apache-2.0](LICENSE-APACHE) 중 선택 (이중 라이선스).
