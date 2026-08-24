# Animation Pipeline

OnlineRPG 클라이언트의 캐릭터 애니메이션 로딩/매핑 규칙 문서.

## 1. 관련 파일

- 캐릭터 베이스 모델: `client/src/lib/utils/modelPaths.ts`의 `getCharacterModelPath(...)`가 반환하는 모델
- 이동 전용 클립: `client/public/models/animations/locomotion.glb`
- 근접 전투 클립: `client/public/models/animations/combat_melee.glb`
- 애니메이션 이름/순서 정의: `client/src/lib/types/animations.ts`
- 공통 유틸: `client/src/lib/utils/characterAnimationUtils.ts`
- 런타임 캐릭터: `client/src/lib/components/PlayerModel.svelte`
- 캐릭터 선택 프리뷰: `client/src/lib/components/CharacterPreview.svelte`

## 2. 표준 클립 이름

아래 이름은 코드에서 직접 참조하므로 대소문자까지 정확히 일치해야 한다.

| Category | Clips | Animation Pack |
|---|---|---|
| Idle | `idle1`, `idle2`, `idle3`, `idle4`, `idle5` | `locomotion` |
| Move | `walk`, `jog`, `run`, `jump` | `locomotion` |
| Attack | `slash1`, `slash2`, `slash3`, `slash4`, `slash5` | `combat_melee` |
| Attack alt | `attack1`, `attack2`, `attack3`, `attack4` | `combat_melee` |
| Death | `dying` | `combat_melee` |
| Attack idle | `combat_idle` | `combat_melee` (몬스터 `animAttackIdle` + 플레이어 스윙 사이 쿨다운) |
| Claw attack | `claw1`, `claw2` | `combat_melee` (몬스터 `animAttack` 전용, 플레이어 미사용) |

순서 기준은 `AnimationName` enum 선언 순서(`client/src/lib/types/animations.ts`)를 따른다.

몬스터 `animAttack`(monsters.csv)은 `claw1|claw2`처럼 `|`로 여러 클립을 나열할 수 있다 — 클라이언트가
스윙마다 하나를 랜덤으로 고르고, 서버의 스윙 홀드 시간(`data/monster_attack_clips.json`,
`tools/measure-monster-attack-clips.mjs`)은 그중 가장 긴 클립을 따른다.

## 2-1. 본 구조 (Mixamo)

현재 휴먼 본 구조는 Mixamo(`mixamorig`) 계열을 따른다.

- `glb-editor` 기준 본 이름: `mixamorig:Hips`, `mixamorig:Spine` 형태
- 현재 export된 node 이름: 접두어/콜론이 제거된 `Hips`, `Spine` 형태

## 2-2. 본 계층 구조 (부모-자식)

![locomotion bone hierarchy](./images/diagrams/animation-bone-hierarchy.svg)

- 파일: `doc/images/diagrams/animation-bone-hierarchy.svg`
- `locomotion.glb`에서 추출한 65개 본의 부모-자식 구조를 시각화한 이미지

## 3. 현재 매핑 정책

`selectOrderedCharacterAnimations(...)`에서 아래 우선순위를 사용한다.

1. `idle1~idle5`, `walk`, `jog`, `run`, `jump`는 `locomotion.glb` 우선
2. `slash1~slash5`, `attack1~attack4`, `dying`은 `combat_melee.glb` 우선
3. 지정된 source에 해당 이름의 클립이 없으면 같은 source의 첫 번째 클립으로 fallback
4. 지정된 source에도 fallback 클립이 없으면 빈 배열을 반환

초기 로딩 시 캐릭터 베이스 모델, `locomotion.glb`, `combat_melee.glb`, 기본 무기 모델 로드를 모두 기다린다.

## 4. 컴포넌트별 사용 방식

### PlayerModel

- 플레이 상태(`idle`, `moving`, `attack`, `dead`) 기반으로 클립 선택
- `moving` 상태에서는 시작 시점에 `walk/jog/run` 중 하나를 lock
- `idle`은 idle 계열 클립 중 랜덤 반복
- `attack`은 `slash1`, `dead`는 `dying` 사용

### CharacterPreview

- 선택 화면에서 idle 계열만 재생
- 선택되지 않은 슬롯은 action pause + time reset

## 5. 자주 발생하는 문제

### `THREE.PropertyBinding: No target node found for track: mixamorigHips.position`

원인:

- 애니메이션 트랙 본 이름과 타깃 모델 본 이름이 다를 때 발생
- 예: `locomotion.glb`는 `mixamorigHips`, 캐릭터 베이스 모델은 `Hips`

대응:

1. `glb-editor`에서 `본 이름 표준화`를 실행한다.

참고: 경고가 발생하면 클립이 부분/전체 미적용될 수 있으므로 반드시 확인한다.

### 무릎이 반대로 꺾인다 (rest 포즈 컨벤션 차이)

원인:

- `SkeletonUtils.retargetClip`은 소스 본의 **월드 회전을 그대로 복사**한다. 두 리그가 본을 같은
  방향으로 롤링해 놨을 때만 맞는 방식이다
- cyclop·lizardfolk·stone_golem은 다른 리거를 거쳐 와서 다리 본이 팩 리그 대비 ~170° 롤링돼 있다
  (본이 가리키는 방향은 같고 축만 뒤집힘 — twist 170°, swing 10°). 절대 회전을 복사하면 다리
  메시가 축을 중심으로 감긴다

대응 (`retargetAnimationsForCharacterModel`이 자동 처리):

- 실루엣 본 16개(힙·척추·목·머리·사지) 중 하나라도 rest 회전이 팩과 120°를 넘게 벌어진 리그에서,
  **그 한계를 넘은 본만** `localOffsets`(`R_source_rest⁻¹ · R_target_rest`)를 받아 절대 포즈 대신
  모션을 옮긴다. 위 3종에서는 다리·발가락 본만 해당
- 나머지 본은 팩의 절대 회전을 그대로 받는다. 28개 리그를 같은 포즈로 세우는 것이 대조 시트의
  전부이고, 한계 아래의 차이는 어느 리그에나 있는 bind 포즈 차이이기 때문이다 —
  combat_melee 기준 팔 각도는 cyclop 54°, troll 67°로 같은 급이다. 리그 전체를 보정했더니
  cyclop·lizardfolk만 칼 잡는 팔이 나머지 26종과 50~86° 어긋났다
- 배포 중인 리그 × 팩 조합에서 정상 리그의 최대치는 86°(gnoll × combat_melee), 문제의 3종
  다리는 최소 170°라 경계가 넓다. 한계를 넘는 본이 없는 리그는 클립이 그대로다 — 현재 게임에서
  리타게팅을 타는 리그(플레이어 16종 + `sharedAnims=true` 몬스터 5종)는 전부 여기에 해당한다
- 손가락은 리그 판정에서 제외한다 — bind 포즈에 따라 리그마다 최대 140°까지 벌어지는데 이건
  컨벤션이 아니라 쥔 모양 차이다

## 6. 신규 애니메이션 추가 체크리스트

1. `glb-editor`에서 `본 이름 표준화` 버튼을 눌러 본 이름을 정리한다.
2. `애니메이션 추출` 버튼을 눌러 애니메이션을 추출한다.
3. 추출한 클립을 애니메이션 팩 중 하나(`locomotion`, `combat_melee`, `social`, `offhand`)에 넣는다.
   - 배포 중인 팩에 넣을 때는 `python tools/graft-glb-clip.py 팩.glb 도너.glb 클립이름 출력.glb`. 기존 클립과 스켈레톤을 바이트 단위로 보존한다.
   - `export_animations.py`로 팩을 통째로 다시 뽑아도 된다(2026-08-13 검증: 5팩 전부 배포본과 채널 단위 일치, 최대 오차 1.4e-5). 단 새 클립을 `all_animation.blend`에 먼저 넣어야 하고, glTF를 임포트해 넣었다면 키를 정수 프레임으로 스냅할 것 — 마지막 키가 `125.99999`로 들어오면 export에서 1프레임이 깎인다.
   - `combat_melee`만 69본 `Armature_combat`(손가락·눈·소매 본)에서 뽑는다. 33본 `Armature`로 뽑으면 채널 절반이 사라지고 rest 포즈가 어긋난다. 요청한 액션이 하나라도 없으면 스크립트가 해당 팩을 abort하고 기존 GLB를 건드리지 않는다.
4. 클립 이름을 `AnimationName`에 추가
5. `AnimationIndex` 동기화
6. 필요한 경우 `selectOrderedCharacterAnimations` 우선순위 반영
7. `PlayerModel` 상태 전이에서 새 클립 사용 지점 연결
8. `CharacterPreview`에서 필요한 경우 재생 정책 반영
9. 실행 검증
   - `cd client && npm run lint`
   - `cd client && npm run check`
   - 게임 내에서 `/anim <클립이름>` (admin 전용, 클라이언트 로컬)으로 클립 단독 재생 확인

## 6-1. 여러 리그에 동시에 걸어 보기 (`tools/anim-preview`)

공용 팩을 갈아 끼울 때, 클립 하나가 28개 리그 전부에서 어떻게 보이는지는 한 화면에서
비교해야 판단이 된다. `tools/anim-preview`가 그 용도다.

- 몬스터 12종(`monsters.csv`의 서로 다른 GLB, 보스는 베이스와 같은 리그라 1회만) +
  캐릭터 16종을 한 시트에 띄우고, 같은 모션을 동시에 재생한다
- 리타게팅은 클라이언트의 `loadSharedPackClipsForModel` /
  `retargetAnimationsForCharacterModel`을 `$game` alias로 직접 호출한다 —
  화면에 보이는 것이 곧 게임이 재생하는 것
- Mixamo 다운로드(`takes/`)를 모션별로 골라 비교하고, 고른 것들을 하나의 GLB 팩으로 export한다
  (파일명 지정, 게임이 로드하는 팩 이름은 거부)
- 코어 본(힙·척추·목·머리·사지 12본)이 빠진 리그는 기본 off, 빠진 본 이름을 함께 표시.
  손가락은 판정에서 제외 — 오거(33본)·놀(57본)도 걷기/공격은 봐야 한다

`loadSharedPackClipsForModel`에 5번째 인자 `packPaths`가 추가됐다. 게임은 넘기지 않으므로
동작은 그대로이고, anim-preview가 후보 팩을 배포본 위에 덮어쓰지 않고 시험할 때만 쓴다.

## 7. 버전 로그

- `v0.10` (2026-08-31): rest 포즈 컨벤션이 다른 리그(cyclop·lizardfolk·stone_golem)의 어긋난 본만 보정하는 규칙 추가
- `v0.9` (2026-08-31): 클립 검증용 admin 명령 `/anim <클립이름>` 추가 (기존 `/emote` 숨김 디버그 클립 대체)
- `v0.9` (2026-08-30): `tools/anim-preview` 섹션 추가, `loadSharedPackClipsForModel`의 `packPaths` 인자 기록
- `v0.8` (2026-02-21): 표준 클립 표에 `Animation Pack` 컬럼 추가
- `v0.7` (2026-02-21): 본 계층 구조 표를 SVG 이미지 첨부 방식으로 변경 (`doc/images/diagrams/animation-bone-hierarchy.svg`)
- `v0.6` (2026-02-21): VSCode 프리뷰 가독성을 위해 본 계층 구조를 Mermaid에서 테이블+인덴트 형식으로 변경
- `v0.5` (2026-02-21): 본 계층 구조를 텍스트 트리에서 Mermaid 다이어그램으로 변경
- `v0.4` (2026-02-21): `locomotion.glb` 본 계층 구조(`부모 ㄴ 자식`) 섹션 추가
- `v0.3` (2026-02-21): 신규 애니메이션 추가 절차에 `glb-editor` 본 정리/Extract/4개 묶음 분류 규칙 추가
- `v0.2` (2026-02-21): Mixamo 본 구조 설명 및 `locomotion.glb` 본 이름 목록 추가
- `v0.1` (2026-02-21): 문서 생성, locomotion 우선 매핑 규칙 및 트러블슈팅 정리
