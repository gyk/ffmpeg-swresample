(ns run
  (:require [babashka.cli :as cli]
            [babashka.process :refer [shell]]
            [clojure.pprint :as pprint]))

(def opt-spec
  {:times   {:coerce   :long
             :ref      "<n>"
             :desc     "#Times"
             :alias    :n
             :validate pos?
             :require  true}
   :command {:ref   "<command>"
             :desc  "The command to run (after `cargo run --`)"
             :alias :c}})

(defn detect-crash
  []
  (let [opts (try
               (cli/parse-opts
                 *command-line-args*
                 {:spec     opt-spec
                  :restrict true})
               (catch Exception _
                 (println (cli/format-opts {:spec opt-spec}))
                 (System/exit 0)))
        n    (:times opts)]
    (try
      (loop [i 0]
        (if (< i n)
          (let [result (shell {:continue true
                               :err      :string}
                              "cargo run --"
                              (:command opts))]
            (if (zero? (:exit result))
              (recur (inc i))
              (do
                (println "⚠️ Crash detected")
                (println "================================\n")
                (println (:err result))
                (println "================================\n")
                (pprint/pprint {:exit-code (:exit result)
                                :iteration i}))))
          (println "No crash detected.")))
      (catch Exception e
        (println "Error occurred:")
        (pprint/pprint (ex-data e))))))
