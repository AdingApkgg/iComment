-- challenge_mode 从布尔化的 turnstile/off 扩成三态：always / suspicious / off
alter table site drop constraint if exists site_challenge_mode_check;
update site set challenge_mode = 'always' where challenge_mode = 'turnstile';
alter table site alter column challenge_mode set default 'always';
alter table site add constraint site_challenge_mode_check
  check (challenge_mode in ('always', 'suspicious', 'off'));
