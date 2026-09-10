import Foundation
import Supabase

/// One shared client for the whole app, same pattern as the web app's `lib/supabase.ts` and the
/// Mac app's `nodeconfig` defaults -- same Supabase project (`pxfbnuxcnerulbvbmowz`) as everything
/// else in Hive. The publishable key is safe to ship in the client bundle (that's its purpose);
/// nothing more sensitive belongs here.
enum SupabaseConfig {
    static let projectURL = URL(string: "https://pxfbnuxcnerulbvbmowz.supabase.co")!
    static let publishableKey = "sb_publishable_VjfocwhBAykEFEllo6U3RQ_e-BdMcme"
}

let supabase = SupabaseClient(supabaseURL: SupabaseConfig.projectURL, supabaseKey: SupabaseConfig.publishableKey)
