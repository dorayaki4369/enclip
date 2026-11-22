"""受信側の重複検出と期限の状態機械を、有限の範囲で全状態探索する。

仕様 4.3, 4.4:
  - 受信: 期限切れなら捨てる。記録にある ID なら捨てる。それ以外は受け入れて ID を期限まで記録する
  - 削除: 期限を過ぎた記録を消す
攻撃者（サーバー）は、送られたアイテムを任意の時点で何度でも配信できる。
性質: 同じアイテムを二度受け入れない。
"""
from collections import deque
import itertools, sys

T_MAX = 6          # 時計の範囲 0..T_MAX
ITEMS = {"a": 3, "b": 5}   # アイテム ID -> 期限（送信側が決め、暗号文の中にある）

def explore(clock_can_go_back: bool, use_high_water: bool):
    # 状態: (時計, 高水位, 記録 frozenset[(id,exp)], 受け入れ回数 tuple)
    init = (0, 0, frozenset(), tuple(0 for _ in ITEMS))
    ids = list(ITEMS)
    seen = {init: None}
    q = deque([init])
    while q:
        st = q.popleft()
        t, hw, rec, cnt = st
        now = hw if use_high_water else t
        succ = []
        # 時計が進む
        if t < T_MAX:
            nt = t + 1
            succ.append(("tick", (nt, max(hw, nt), rec, cnt)))
        # 時計が戻る（利用者の時刻変更、NTP 補正など）
        if clock_can_go_back:
            for nt in range(0, t):
                succ.append((f"set_clock({nt})", (nt, hw, rec, cnt)))
        # 期限を過ぎた記録を消す
        nrec = frozenset((i, e) for (i, e) in rec if now < e)
        if nrec != rec:
            succ.append(("purge", (t, hw, nrec, cnt)))
        # サーバーがアイテムを配信する（再送を含む）
        for k, i in enumerate(ids):
            e = ITEMS[i]
            if now >= e:
                continue                      # 期限切れは捨てる
            if any(ri == i for (ri, _) in rec):
                continue                      # 記録にある ID は捨てる
            nc = list(cnt); nc[k] += 1
            succ.append((f"deliver({i})", (t, hw, rec | {(i, e)}, tuple(nc))))
        for label, ns in succ:
            if ns not in seen:
                seen[ns] = (st, label)
                if max(ns[3]) >= 2:
                    path = []
                    cur = ns
                    while seen[cur] is not None:
                        prev, lab = seen[cur]
                        path.append(lab); cur = prev
                    return len(seen), list(reversed(path))
                q.append(ns)
    return len(seen), None

for back, hw in [(False, False), (True, False), (True, True)]:
    n, cex = explore(back, hw)
    name = f"時計が戻る={back}, 高水位の時刻を使う={hw}"
    print(f"{name}: 状態数 {n}, " + ("二重受け入れなし" if cex is None else "反例: " + " -> ".join(cex)))
