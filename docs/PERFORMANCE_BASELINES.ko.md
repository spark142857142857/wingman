# 성능 기준 측정 기록

영문판: [PERFORMANCE_BASELINES.md](PERFORMANCE_BASELINES.md)

이 문서는 재현 가능한 component 측정 결과를 보관한다. Component 결과는 필요한
검사이지만 [성능 예산](PERFORMANCE_BUDGET.ko.md)의 Wingman 전체 process-tree 배포
게이트를 대신하지 않는다.

## 2026-08-11: idle `tail -f` runner component

- Commit: `2901cba79180d8eab99cd5df1b41c2e110b7ef93`
- OS: Microsoft Windows 11 Home `10.0.26200` (build `26200`)
- CPU: AMD Ryzen 7 9700X, physical 8 core, logical processor 16개
- 전원: Windows 균형 조정 (`381b4222-f694-41f0-9685-ff5bb260df2e`)
- Toolchain: `rustc 1.96.1`, `cargo 1.96.1`
- Build: 최적화된 Cargo `release`

실행 명령:

```powershell
cargo test --release --test runner_process_contract idle_tail_follow_runner_stays_below_the_cpu_ceiling -- --ignored --exact --nocapture
```

Test는 실제 broker를 통해 `wingman-runner`를 시작하고 변경 없는 빈 파일에
`tail -n 0 -f`를 실행한다. 10초 안정화 뒤 1초 process CPU-time 표본 10개를 수집해
logical processor 16개 기준으로 정규화하고, 정상 process-group 취소를 보내 종료
`130`을 요구한다.

| 독립 실행 | Median CPU | p95 CPU | 결과 |
| --- | ---: | ---: | --- |
| 1 | 0.000% | 0.000% | 통과 |
| 2 | 0.000% | 0.000% | 통과 |
| 3 | 0.000% | 0.000% | 통과 |

모든 1초 delta가 process CPU-time 측정 해상도보다 작았다. 따라서 이 component는
배포 상한 median 0.5%, p95 2%를 통과한다. 전체 app·WebView2·PTY·shell·child process
tree는 reference matrix에서 별도 ETW/WPR 배포 측정을 수행해야 한다.
