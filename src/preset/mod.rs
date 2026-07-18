//! Human-readable preset format (`.osc` files).
//!
//! A preset is a small sound program containing the Oscilla waveform script
//! and synth parameters. The format is designed to be readable, writeable,
//! and shareable.
//!
//! Example:
//! ```text
//! name "Warm Pad"
//! wave { math.sin(x) + math.sin(x*2)*0.5 + math.sin(x*4)*0.25 }
//! filter { cutoff 1200 resonance 0.3 type lp }
//! envelope { attack 0.05 decay 0.4 sustain 0.7 release 1.2 }
//! unison { voices 3 detune 8 width 0.6 }
//! volume 0.75
//! glide 0.03
//! ```

use crate::dsp::filter::FilterType;

/// A complete Oscilla preset.
#[derive(Debug, Clone)]
pub struct Preset {
    pub name: String,
    pub wave_script: String,
    pub filter_type: FilterType,
    pub filter_cutoff: f32,
    pub filter_resonance: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub unison_voices: usize,
    pub detune_cents: f32,
    pub stereo_width: f32,
    pub volume: f32,
    pub glide: f32,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: "Init".into(),
            wave_script: "math.sin(x)".into(),
            filter_type: FilterType::LowPass,
            filter_cutoff: 20000.0, // wide open
            filter_resonance: 0.0,
            attack: 0.01,
            decay: 0.2,
            sustain: 0.7,
            release: 0.4,
            unison_voices: 1,
            detune_cents: 10.0,
            stereo_width: 0.5,
            volume: 0.8,
            glide: 0.05,
        }
    }
}

/// Parse error.
#[derive(Debug)]
pub enum ParseError {
    UnexpectedToken(String),
    MissingValue(String),
    InvalidValue(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedToken(s) => write!(f, "unexpected token: {s}"),
            Self::MissingValue(s) => write!(f, "missing value for: {s}"),
            Self::InvalidValue(s) => write!(f, "invalid value: {s}"),
        }
    }
}

/// A very simple tokenizer for .osc format.
struct Tokenizer<'a> {
    chars: std::str::Chars<'a>,
    peeked: Option<char>,
}

impl<'a> Tokenizer<'a> {
    fn new(s: &'a str) -> Self {
        let mut chars = s.chars();
        let peeked = chars.next();
        Self { chars, peeked }
    }

    fn peek(&self) -> Option<char> {
        self.peeked
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.peeked;
        self.peeked = self.chars.next();
        c
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.next_char();
                }
                Some('#') | Some('/') => {
                    // Line comment
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.next_char();
                    }
                }
                _ => break,
            }
        }
    }

    fn read_word(&mut self) -> Option<String> {
        self.skip_whitespace_and_comments();
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                s.push(c);
                self.next_char();
            } else {
                break;
            }
        }
        if s.is_empty() { None } else { Some(s) }
    }

    fn read_quoted_string(&mut self) -> Result<String, ParseError> {
        self.skip_whitespace_and_comments();
        match self.peek() {
            Some('"') => {
                self.next_char(); // consume opening quote
                let mut s = String::new();
                loop {
                    match self.next_char() {
                        Some('"') => break,
                        Some(c) => s.push(c),
                        None => {
                            return Err(ParseError::UnexpectedToken("unterminated string".into()));
                        }
                    }
                }
                Ok(s)
            }
            Some(c) => Err(ParseError::UnexpectedToken(format!(
                "expected string, got '{c}'"
            ))),
            None => Err(ParseError::UnexpectedToken("unexpected EOF".into())),
        }
    }

    fn read_number(&mut self) -> Result<f32, ParseError> {
        self.skip_whitespace_and_comments();
        let mut s = String::new();
        // Allow leading minus and digits/dots.
        if let Some('-') = self.peek() {
            s.push('-');
            self.next_char();
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                s.push(c);
                self.next_char();
            } else {
                break;
            }
        }
        s.parse::<f32>()
            .map_err(|_| ParseError::InvalidValue(format!("not a number: '{s}'")))
    }

    fn read_block(&mut self) -> Result<String, ParseError> {
        self.skip_whitespace_and_comments();
        match self.peek() {
            Some('{') => {
                self.next_char(); // consume opening brace
                let mut depth = 1;
                let mut s = String::new();
                while depth > 0 {
                    match self.next_char() {
                        Some('{') => {
                            depth += 1;
                            s.push('{');
                        }
                        Some('}') => {
                            depth -= 1;
                            if depth > 0 {
                                s.push('}');
                            }
                        }
                        Some(c) => s.push(c),
                        None => {
                            return Err(ParseError::UnexpectedToken("unterminated block".into()));
                        }
                    }
                }
                Ok(s.trim().to_string())
            }
            Some(c) => Err(ParseError::UnexpectedToken(format!(
                "expected '{{', got '{c}'"
            ))),
            None => Err(ParseError::UnexpectedToken("unexpected EOF".into())),
        }
    }
}

impl Preset {
    /// Parse a .osc preset from a string.
    pub fn parse(source: &str) -> Result<Self, ParseError> {
        let mut t = Tokenizer::new(source);
        let mut preset = Preset::default();

        while let Some(word) = t.read_word() {
            match word.as_str() {
                "name" => {
                    preset.name = t.read_quoted_string()?;
                }
                "wave" => {
                    preset.wave_script = t.read_block()?;
                }
                "filter" => {
                    let block = t.read_block()?;
                    let mut ft = Tokenizer::new(&block);
                    while let Some(kw) = ft.read_word() {
                        match kw.as_str() {
                            "cutoff" => preset.filter_cutoff = ft.read_number()?,
                            "resonance" => preset.filter_resonance = ft.read_number()?,
                            "type" => {
                                if let Some(tw) = ft.read_word() {
                                    preset.filter_type = match tw.as_str() {
                                        "lp" | "lowpass" => FilterType::LowPass,
                                        "hp" | "highpass" => FilterType::HighPass,
                                        "bp" | "bandpass" => FilterType::BandPass,
                                        _ => {
                                            return Err(ParseError::InvalidValue(format!(
                                                "unknown filter type: {tw}"
                                            )));
                                        }
                                    };
                                }
                            }
                            _ => {
                                return Err(ParseError::UnexpectedToken(format!(
                                    "unknown filter key: {kw}"
                                )));
                            }
                        }
                    }
                }
                "envelope" => {
                    let block = t.read_block()?;
                    let mut ft = Tokenizer::new(&block);
                    while let Some(kw) = ft.read_word() {
                        match kw.as_str() {
                            "attack" => preset.attack = ft.read_number()?,
                            "decay" => preset.decay = ft.read_number()?,
                            "sustain" => preset.sustain = ft.read_number()?,
                            "release" => preset.release = ft.read_number()?,
                            _ => {
                                return Err(ParseError::UnexpectedToken(format!(
                                    "unknown envelope key: {kw}"
                                )));
                            }
                        }
                    }
                }
                "unison" => {
                    let block = t.read_block()?;
                    let mut ft = Tokenizer::new(&block);
                    while let Some(kw) = ft.read_word() {
                        match kw.as_str() {
                            "voices" => {
                                preset.unison_voices = ft.read_number()? as usize;
                            }
                            "detune" => preset.detune_cents = ft.read_number()?,
                            "width" => preset.stereo_width = ft.read_number()?,
                            _ => {
                                return Err(ParseError::UnexpectedToken(format!(
                                    "unknown unison key: {kw}"
                                )));
                            }
                        }
                    }
                }
                "volume" => preset.volume = t.read_number()?.clamp(0.0, 1.0),
                "glide" => preset.glide = t.read_number()?.max(0.0),
                other => {
                    return Err(ParseError::UnexpectedToken(format!(
                        "unknown section: {other}"
                    )));
                }
            }
        }

        Ok(preset)
    }

    /// Serialize the preset to a human-readable string.
    pub fn to_string(&self) -> String {
        let ft_str = match self.filter_type {
            FilterType::LowPass => "lp",
            FilterType::HighPass => "hp",
            FilterType::BandPass => "bp",
        };
        format!(
            "name \"{name}\"\n\n\
             wave {{\n\
                 {wave}\n\
             }}\n\n\
             filter {{\n\
                 cutoff     {cutoff:>8.1}\n\
                 resonance  {res:>8.2}\n\
                 type       {ft:>8}\n\
             }}\n\n\
             envelope {{\n\
                 attack     {atk:>8.3}\n\
                 decay      {dec:>8.3}\n\
                 sustain    {sus:>8.2}\n\
                 release    {rel:>8.3}\n\
             }}\n\n\
             unison {{\n\
                 voices     {uv:>8}\n\
                 detune     {det:>8.0}\n\
                 width      {width:>8.2}\n\
             }}\n\n\
             volume     {vol:>8.2}\n\
             glide      {glide:>8.3}\n",
            name = self.name,
            wave = self.wave_script,
            cutoff = self.filter_cutoff,
            res = self.filter_resonance,
            ft = ft_str,
            atk = self.attack,
            dec = self.decay,
            sus = self.sustain,
            rel = self.release,
            uv = self.unison_voices,
            det = self.detune_cents,
            width = self.stereo_width,
            vol = self.volume,
            glide = self.glide,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_default_preset() {
        let source = r#"
            name "Test"
            wave { math.sin(x) + math.sin(x*3)*0.25 }
            filter { cutoff 800 resonance 0.5 type lp }
            envelope { attack 0.01 decay 0.2 sustain 0.7 release 0.4 }
            unison { voices 2 detune 10 width 0.5 }
            volume 0.8
            glide 0.05
        "#;
        let preset = Preset::parse(source).unwrap();
        assert_eq!(preset.name, "Test");
        assert_eq!(preset.wave_script, "math.sin(x) + math.sin(x*3)*0.25");
        assert_eq!(preset.filter_cutoff, 800.0);
        assert_eq!(preset.unison_voices, 2);
        assert_eq!(preset.volume, 0.8);
    }

    #[test]
    fn test_roundtrip() {
        let preset = Preset::default();
        let s = preset.to_string();
        let parsed = Preset::parse(&s).unwrap();
        assert_eq!(parsed.name, preset.name);
        assert_eq!(parsed.wave_script, preset.wave_script);
        assert!((parsed.volume - preset.volume).abs() < 0.001);
    }
}
