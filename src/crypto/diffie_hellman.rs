use rand::RngExt;
use uint::construct_uint;

construct_uint!{
    pub struct U576(9);
}

#[derive(Clone, PartialEq, Debug)]
pub struct Point(U576, U576);

impl From<(&str, &str)> for Point {
    fn from(value: (&str, &str)) -> Self {
        Point(U576::from_dec_str(value.0).unwrap(), U576::from_dec_str(value.1).unwrap())
    }
}

impl ToString for Point {
    fn to_string(&self) -> String {
        format!("{};{}", self.0, self.1)
    }
}

impl Point {
    pub fn get_x(&self) -> U576 {
        self.0
    }

    pub fn get_y(&self) -> U576 {
        self.1
    }
}

#[derive(Clone)]
struct Expression((U576, U576), (U576, U576));

pub struct EllipticCurve(U576, U576, U576);

impl EllipticCurve {
    pub fn add_points(&self, point1: &Point, point2: &Point) -> Point {
        // Destruct the curve and the points and modulo points with p
        let &EllipticCurve(a, _, p) = self;
        let &Point(mut x1, mut y1) = point1;
        let &Point(mut x2, mut y2) = point2;
        (x1, y1, x2, y2) = (x1 % p, y1 % p, x2 % p, y2 % p);
        
        // Get the slope between the two points
        let lambda = if point1 == point2 {
            // lambda ≡ (3*x1^2 + a) / (2 * y1) (mod p)
            let numerator = ((U576::from(3) * ((x1 * x1) % p)) % p + a) % p;
            let denominator = (U576::from(2) * y1) % p;
            (numerator * mod_mult_inverse(denominator, p)) % p
        } else {
            // lambda ≡ (y2 - y1) / (x2 - x1) (mod p)
            let numerator = y2 + (p - y1) % p;
            let denominator = x2 + (p - x1) % p;
            (numerator * mod_mult_inverse(denominator, p)) % p
        };

        let x_next = (lambda.pow(U576::from(2)) + (p - x1) + (p - x2)) % p;
        let y_next = (lambda * (x1 + (p - x_next)) + (p - y1)) % p;

        Point(x_next, y_next)
    }

    pub fn get_point_from(&self, mut point: Point, target: U576) -> Point {
        let mut pos = U576::from(1);
        let mut points: Vec<(U576, Point)> = vec![(pos, point.clone())];

        // Keep taking largest steps without overstepping
        while pos != target {
            let last_point = points.last().unwrap();

            for prev_point in points.iter().rev() {
                let pos_sum = last_point.0 + prev_point.0;
                if pos_sum <= target {
                    point = self.add_points(&last_point.1, &prev_point.1);
                    pos = pos_sum;
                    points.push((pos, point.clone()));
                    break;
                }
            }
        }

        point
    }

    pub fn valid_point(&self, point: &Point) -> bool {
        let &EllipticCurve(a, b, p) = self;
        let &Point(x, y) = point;

        let lhs = (y * y) % p;
        let rhs = ((x * x % p) * x % p + a * x + b) % p;
        lhs == rhs
    }
}

fn mod_mult_inverse(n: U576, modulus: U576) -> U576 {
    // Step 1: Find GCD using Euclidean Algorithm
    let mut lines = vec![];
    let mut gcd = n;
    let mut lhs = modulus;

    loop {
        let (div, rem) = lhs.div_mod(gcd);
        if rem == U576::from(0) {
            break;
        }
        lines.push(Expression((U576::from(1), lhs), (div, gcd)));
        lhs = gcd;
        gcd = rem;
    }

    if gcd != U576::from(1) {
        panic!("No modular multiplicative inverse exists!")
    }

    // Step 2: Rewrite to Bezout identity
    let mut bezout_identity = lines.pop().unwrap();

    lines.into_iter().rev().for_each(|intermediate| {
        let Expression((a, x), (b, y)) = bezout_identity;
        let Expression((c, z), (d, w)) = intermediate;

        let (a_, x_, b_, y_) = if x == w {
            // (a + bd)x - (bc)z
            (a+b*d, x, b*c, z)
        } else {
            // (ac)z - (ad + b)y
            (a*c, z, a*d+b, y)
        };

        bezout_identity = Expression((a_, x_), (b_, y_));
    });

    // Step 3: Infer modular multiplicative inverse
    let Expression((a, _), (b, y)) = bezout_identity;

    if y == n {
        return modulus - b;
    }

    a
}

pub fn get_random_uint() -> U576 {
    let digits = 77;

    let char_buffer: String = (0..digits).map(|_| rand::rng().random_range('0'..='9')).collect();

    U576::from_dec_str(char_buffer.as_str()).unwrap()
}

pub fn get_elliptic_curve() -> EllipticCurve {
    let p = U576::from(2).pow(U576::from(256)) - U576::from(2).pow(U576::from(32)) - U576::from(977);
    let a = U576::from(0);
    let b = U576::from(7);
    EllipticCurve(a, b, p)
}

pub fn get_generator_point() -> Point {
    Point(
        U576::from_dec_str("55066263022277343669578718895168534326250603453777594175500187360389116729240").unwrap(),
        U576::from_dec_str("32670510020758816978083085130507043184471273380659243275938904335757337482424").unwrap()
    )
}