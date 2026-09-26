// verify: release ok
// Hot/cold splitting vs structure-of-arrays, for two access patterns (Chapter 5.2's promise, Chapter 9.5's incident).
// 500K accounts. The "report" scans one hot field of every account; the "payment" touches the hot fields AND the cold
// record of one random account. Three layouts:
//   AoS:       Vec<Account> with the 112-byte cold part inline (128 bytes per account)
//   hot/cold:  Vec<Hot> (16 bytes) + Vec<Cold> (112 bytes), same index
//   SoA:       one Vec per hot field + Vec<Cold>
// One Playground run, noisy.
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy)]
#[allow(dead_code)] // `address` is the cold payload: it exists to take up space, like the real record
struct Cold {
    kyc_ref: [u8; 48],
    address: [u8; 64],
}

#[derive(Clone, Copy)]
struct Account {
    balance: i64,
    limit: i64,
    cold: Cold,
}

#[derive(Clone, Copy)]
struct Hot {
    balance: i64,
    limit: i64,
}

struct Soa {
    balance: Vec<i64>,
    limit: Vec<i64>,
    cold: Vec<Cold>,
}

fn ms(mut f: impl FnMut() -> i64) -> f64 {
    (0..5)
        .map(|_| {
            let t = Instant::now();
            black_box(f());
            t.elapsed().as_secs_f64() * 1e3
        })
        .fold(f64::MAX, f64::min)
}

fn main() {
    let n = 500_000;
    let cold = Cold { kyc_ref: [1; 48], address: [2; 64] };
    let aos: Vec<Account> = (0..n).map(|i| Account { balance: i as i64, limit: 1000, cold }).collect();
    let hot: Vec<Hot> = (0..n).map(|i| Hot { balance: i as i64, limit: 1000 }).collect();
    let colds: Vec<Cold> = vec![cold; n];
    let soa = Soa { balance: (0..n as i64).collect(), limit: vec![1000; n], cold: vec![cold; n] };
    println!("bytes per account: AoS {}, hot {}, cold {}", size_of::<Account>(), size_of::<Hot>(), size_of::<Cold>());

    // Report: sum of balances over every account.
    let r_aos = ms(|| black_box(&aos).iter().map(|a| a.balance).sum());
    let r_hc = ms(|| black_box(&hot).iter().map(|h| h.balance).sum());
    let r_soa = ms(|| black_box(&soa.balance).iter().sum());
    println!("report scan (500K balances), ms:       AoS {r_aos:6.2}   hot/cold {r_hc:6.2}   SoA {r_soa:6.2}");

    // Payment: 1M random accounts; read balance and limit, and one byte of the cold record (the KYC check).
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let idx: Vec<usize> = (0..1_000_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % n as u64) as usize
        })
        .collect();
    let p_aos = ms(|| {
        let a = black_box(&aos);
        idx.iter().map(|&i| a[i].balance - a[i].limit + a[i].cold.kyc_ref[0] as i64).sum()
    });
    let p_hc = ms(|| {
        let (h, c) = (black_box(&hot), black_box(&colds));
        idx.iter().map(|&i| h[i].balance - h[i].limit + c[i].kyc_ref[0] as i64).sum()
    });
    let p_soa = ms(|| {
        let s = black_box(&soa);
        idx.iter().map(|&i| s.balance[i] - s.limit[i] + s.cold[i].kyc_ref[0] as i64).sum()
    });
    println!("payments (1M random accounts), ms:     AoS {p_aos:6.2}   hot/cold {p_hc:6.2}   SoA {p_soa:6.2}");
}
