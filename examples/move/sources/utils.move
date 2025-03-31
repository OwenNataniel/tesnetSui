module walrus::utils;

/// Returns true if `prefix` is a prefix of `word`.
public(package) fun is_prefix(prefix: vector<u8>, word: vector<u8>): bool {
    if (prefix.length() > word.length()) {
        return false
    };
    let mut i = 0;
    while (i < prefix.length()) {
        if (word[i] != prefix[i]) {
            return false
        };
        i = i + 1;
    };
    true
}
