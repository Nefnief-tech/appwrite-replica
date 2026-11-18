-- Editable auth email templates (server-wide, like the SMTP settings).
-- Placeholders: {{link}}, {{code}}, {{email}}, {{appName}}.

create table if not exists ow_mail_templates (
    kind text primary key,
    subject text not null,
    body text not null,
    updated_at timestamptz not null default now()
);

insert into ow_mail_templates (kind, subject, body) values
    ('recovery', 'Reset your Semester password',
     'Open this link within an hour to choose a new password:' || chr(10) || chr(10) ||
     '{{link}}' || chr(10) || chr(10) ||
     'If you did not ask for this, ignore this email.' || chr(10)),
    ('verification', 'Verify your Semester email',
     'Confirm your email address:' || chr(10) || chr(10) || '{{link}}' || chr(10)),
    ('mfa', 'Your Semester sign-in code',
     'Your sign-in code is {{code}}. It expires in 10 minutes.' || chr(10))
on conflict (kind) do nothing;
