# 손 편집

[English](hand-edits.md) | **한국어**

화면의 **디자인** 패널 (편집 권한) 에서 요소 뒤의 코드를 직접 바꿈: 텍스트 · variant · 간격 · 형제
사이 순서. 변경 1회 = 임시 수정안 위의 source 1곳 수정. 수정안은 다른 수정안과 같이 검토
([workflow](workflow.ko.md)).

## 사용

1. 화면을 열고 side 패널에서 **디자인** 선택. *Work on* 에서 내 수정안 또는 **새 수정안** 선택
   (첫 변경 때 화면 이름으로 생성).
2. 화면의 요소 클릭. 바꿀 수 있는 것만 표시:
   - **텍스트** — 코드에 직접 쓴 요소 텍스트.
   - **variant**, **size** … — component 의 `cva` variant (shadcn `Button`, `Badge`).
   - **간격** — Tailwind padding · margin · gap class (`p-4`, `gap-x-2` …) 또는 전체 **class**.
     `size` variant 가 있는 component 는 padding 을 size 로 조정.
   - **순서** — 앞 · 뒤 요소와 자리 바꿈.
3. 변경마다 수정안에 저장 후 merge 와 같은 **검사**: 수정안 build + 변경이 닿는 화면에 새 문제
   없음. 화면이 깨지면 이유와 함께 **자동 되돌림**. 작업 중 화면은 `main` 대신 수정안 render.
4. **실행 취소** / **다시 실행** 으로 이번 작업의 변경 단계 이동 (그 사이 파일을 바꾼 사람이 없을
   때). 준비되면 **검토 요청**.

## 바꾸지 않는 것

코드가 계산하는 값 — data 에서 온 텍스트, `variant={on ? "default" : "outline"}`, `cn(...)` 으로
만든 class, 목록에서 온 children — 은 손 편집 대상 아님: 패널에 표시되고 **agent 에게 요청** 으로
[web agent](agent.ko.md) 에 넘김. 자유 배치 (끌어다 놓기) 미지원. 목록 (`.map(...)`) 안에서는 모든
항목에 적용 (경고 표시).

화면에서 쓴 component 는 쓴 위치에서 수정 (**이 화면만**). 요소가 공용 component 안에 쓰여 있으면
**원본 component** 에서 그 파일 수정, 그 파일로 그려지는 화면 수 표시.

## 동작 방식

- `@charet/sdk/vite` 가 host 요소에 `data-src`, component 사용 위치에 `data-charet-use`
  (`path:line:col`) 표시. render 의 요소 지도가 둘 다 보관.
- server 가 수정안 head 의 파일을 읽어 (oxc) 정해진 규칙으로 1곳 수정 (`crates/design`), 결과도
  parse 되어야 함. 변경 1회 = 수정안 commit 1개.
- 검사는 merge 검사 재사용: build → 변경이 닿는 화면을 `main` 캡처와 비교. 실패 시 파일을 이전
  상태로 push.

아직 없음: 변경마다 프로젝트 type 검사 · shadcn lint, 색 · 글꼴, Components 패널에서 넣기, Vue ·
Svelte.
