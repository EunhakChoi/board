# 변경 흐름

[English](workflow.md) | **한국어**

`main` = 모두가 보는 현재안. 모든 변경은 옆의 branch (수정안) 에서 시작, 검토 후 새 버전으로
`main` 에 채택. 채택 없이 `main` 변경 불가, 모든 버전은 되돌리기 가능.

```
push ─▶ 임시 수정안 ─▶ 검토 중 ─▶ 채택 검사 ─▶ main 의 버전 vN
            │             │
            └─────────────┴──▶ 보관 (직접, 또는 14일 방치된 임시 수정안) ─▶ 다시 열기
```

## 임시 수정안 · 수정안

- `main` checkout 에서 `charet push -m "…"` → 변경을 담은 **임시 수정안** 생성, checkout 이 그
  수정안을 따라감. 이후 push 는 같은 수정안에 추가. push 출력에 수정안 link 표시.
- 임시 수정안 = 작성자 소유. 작성자 (또는 관리자) 만 push · 검토 요청 · 보관 · 다시 열기. 다른
  사람은 **복제** 해서 자기 것으로 이어감.
- `charet branch review` (또는 수정안 화면의 **검토 요청**) → 팀에 공개. 편집 권한 누구나 push ·
  보관 · 채택 가능, 팀에 알림.
- `main` 이 바뀌면 열린 수정안은 자동으로 다시 맞춤. 양쪽에서 바뀐 파일은 충돌 표시만 함. 해결은
  작성자 몫 (`charet branch catch-up`, `charet pull`, 수정, push).
- 14일 동안 손대지 않은 임시 수정안은 보관. 작성자가 다시 열 수 있음.

## 수정안 보기

수정안 화면 (**Branches** 탭):

- **Screens**: 화면 · story 별 전후 비교 (나란히 · 겹쳐). 변경이 닿는 화면은 **바뀜**, 새 화면은
  **새로 생김** 표시.
- 수정안 화면의 **댓글** (**Comment** 후 위치 클릭). 수정안에만 남고 `main` 에는 안 보임.
  `charet comments --branch N` 으로 조회.
- **Code**: 바뀐 파일의 줄 단위 diff.
- **History**: push · 검토 요청 · 다시 맞춤 · 충돌 · 승인 기록.

## 채택

**채택** (`charet branch merge`, 혼자 작업 시 `charet push --merge`) 은 편집 권한 필요. `main`
변경 전 확인 순서:

1. **충돌** 이 있으면 중단. 다시 맞추고 파일 정리 먼저.
2. **보호 파일** 은 관리자 승인 필요: package manifest · lockfile, `vite.config.*`,
   `tsconfig*.json`, `postcss.config.*`, `tailwind.config.*`, `.npmrc`, `charet.json`. 설치 ·
   build 때 code 를 실행하므로 관리자 승인 (**보호 파일 승인**, `charet branch approve`) 전까지
   수정안 미리보기는 `main` 의 해당 파일 사용. 관리자의 채택은 승인 포함. 이후 push 에서 다시
   바뀌면 재승인 필요.
3. **채택 검사**: 채택 결과를 build 하고 변경이 닿는 화면을 열어봄. `main` 에 없던 새 문제가
   있으면 채택 보류, 수정안 화면에 목록 표시. build 동안 웹 · CLI 는 대기.

채택 결과 = 새 버전 (`merge`, 수정안 link 포함). 작성자에게 알림, 다른 수정안의 충돌은 각
작성자에게 알림.

## 버전 · 되돌리기

**Versions** 에 모든 버전 표시. 버전의 **Restore** (또는 `charet restore vN`) → 그 버전을 되돌리는
새 버전 생성. 기록은 지우지 않음.

## product repo

charet 은 앱의 사본을 따로 가짐. 버전을 product repo 에 넣는 일 (반영) 과 repo 자체 변경을 다시
들여오는 일 (가져오기) 은 관리자 CLI 로만 함. 웹 도구에는 결과만 표시.

- **기준점**: repo commit 과 일치한다고 알려진 최신 버전 (마지막 가져오기, 또는 마지막으로 반영
  표시한 버전).
- **반영**: product repo 의 작업 branch 에서 `charet handoff export -p <project> --out p.patch` →
  기준점 이후 `main` 변경 (`.charet/` · `charet.json` 제외) 저장. `git apply p.patch` → repo 검사 →
  PR. merge 후 `charet handoff mark vN -p <project>` → vN 과 이전 버전 모두 그 commit 에 반영됨으로
  기록. **Versions** 에 표시.
- **drift**: repo 안에서 `charet handoff -p <project>` (또는 `charet status -p <project>`) → 기준점
  이후 가져온 파일을 바꾼 repo commit · 미commit 변경 목록.
- **다시 가져오기**: `charet import --update -p <project>` → repo 의 파일을 `import` 버전으로 가져옴.
  기준점 위에 올린 뒤 `main` 과 합치므로 아직 반영 안 된 채택분은 유지. 양쪽에서 바뀐 파일은 거부.
  열린 수정안은 채택 때처럼 자동으로 다시 맞춤.

## 설정

| 설정 | 기본값 | |
|---|---|---|
| `CHARET_MERGE_CHECK` | `true` | `main` 변경 전 채택 결과 build · 검사 (미리보기 없는 server 는 `false`) |
